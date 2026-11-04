// Copyright © 2026 Jalapeno Labs

//! A Redis cache of what Kratos and Hydra answer about a secret, which a revocation takes back
//! at once.
//!
//! Each answer is cached under a hash of the secret it was asked about, a session cookie or an
//! access token, so Redis never holds one that works. Nothing that revokes knows those secrets,
//! though: an admin signs a *person* out, a person disconnects an *OAuth client*. So every entry
//! is also filed under its [`Owner`]s, one Redis set per owner, and [`RevocableCache::revoke`]
//! drops every entry an owner's set names.
//!
//! Filing alone leaves one race. A request that asked Kratos just before a revocation landed can
//! cache the answer just after the owner's entries were dropped, and the revoked secret would
//! then work until that entry expired. So a revocation also marks its owner for
//! [`REVOCATION_GRACE`], longer than any call to Kratos or Hydra may take, and an answer is
//! cached only while none of its owners is marked. The check and the write are one Lua script
//! ([`PUT_SCRIPT`]), so no revocation can fall between them; the mark is set before the owner's
//! set is read, so an entry cached before it is always found.
//!
//! The cache is an optimization, never a gate. A read that fails is a miss, a write that fails is
//! skipped, and a revocation that fails leaves Kratos or Hydra as they were told; each is logged,
//! and a request always falls back to asking. Keys, under one namespace per use:
//!
//! - `<namespace>:entry:<sha256 of the secret>`: the answer, expiring with its lifetime.
//! - `<namespace>:owner:<owner>`: the set of entry keys filed under an owner, expiring with the
//!   longest-lived of them.
//! - `<namespace>:revoked:<owner>`: the mark a revocation leaves, for [`REVOCATION_GRACE`].

use std::fmt::Display;
use std::sync::LazyLock;
use std::time::Duration;

use redis::aio::ConnectionManager;
use redis::{AsyncCommands, RedisResult, Script};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use tracing::{Level, event};

/// How long a revocation keeps its owner's answers out of the cache.
///
/// It only has to outlast the calls to Kratos and Hydra in flight when the revocation lands,
/// since only those can return an answer from before it. Each is cut off after ten seconds or
/// less (`REQUEST_TIMEOUT` in `super::kratos` and `crate::oauth::hydra`); a minute leaves room
/// for a slow Redis on top. Longer costs little: the owner's requests still work meanwhile, each
/// asking Kratos or Hydra itself.
pub const REVOCATION_GRACE: Duration = Duration::from_secs(60);

/// Caches `ARGV[1]` under `KEYS[1]` for `ARGV[2]` seconds and files it under each owner, unless
/// an owner was revoked within the grace. `KEYS[2]` onwards are the owners in pairs: an owner's
/// set, then its revocation mark. Answers 1 when it cached, 0 when a mark kept it out.
///
/// An owner's set expires with the longest-lived entry filed in it, so it never outlives what it
/// names by much, and a revocation always finds every entry still cached.
const PUT_SCRIPT: &str = r"
for index = 2, #KEYS, 2 do
  if redis.call('EXISTS', KEYS[index + 1]) == 1 then
    return 0
  end
end
local lifetime = tonumber(ARGV[2])
redis.call('SET', KEYS[1], ARGV[1], 'EX', lifetime)
for index = 2, #KEYS, 2 do
  redis.call('SADD', KEYS[index], KEYS[1])
  if redis.call('TTL', KEYS[index]) < lifetime then
    redis.call('EXPIRE', KEYS[index], lifetime)
  end
end
return 1
";

/// [`PUT_SCRIPT`], hashed once: Redis is sent the hash, and the script itself only the first
/// time a server has not seen it.
static PUT: LazyLock<Script> = LazyLock::new(|| Script::new(PUT_SCRIPT));

/// What an entry is filed under, so revoking it finds the entry: a person, an OAuth client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner(String);

impl Owner {
    /// The owner `id` of `kind`, such as `identity` and a Kratos identity's id. The kind keeps
    /// owners of different kinds apart in one cache.
    pub fn new(kind: &str, id: &impl Display) -> Self {
        Self(format!("{kind}:{id}"))
    }
}

/// One use of the cache, such as browser sessions, under its own namespace of keys.
#[derive(Debug, Clone, Copy)]
pub struct RevocableCache {
    namespace: &'static str,
}

impl RevocableCache {
    pub const fn new(namespace: &'static str) -> Self {
        Self { namespace }
    }

    /// The answer cached for `secret`, if there is one. A cache that cannot be read is logged and
    /// answers nothing, so the caller asks for itself.
    pub async fn get<Answer: DeserializeOwned>(
        &self,
        redis: &ConnectionManager,
        secret: &str,
    ) -> Option<Answer> {
        let mut redis = redis.clone();
        let cached = match redis.get::<_, Option<String>>(self.entry_key(secret)).await {
            Ok(cached) => cached?,
            Err(error) => {
                event!(
                    name: "auth.cache.read_failure",
                    Level::WARN,
                    cache.namespace = self.namespace,
                    error.message = %error,
                    "the cache is unreachable; asking instead",
                );
                return None;
            }
        };
        match serde_json::from_str(&cached) {
            Ok(answer) => Some(answer),
            Err(error) => {
                event!(
                    name: "auth.cache.unreadable",
                    Level::WARN,
                    cache.namespace = self.namespace,
                    error.message = %error,
                    "a cached answer did not parse; asking instead",
                );
                None
            }
        }
    }

    /// Caches `answer` for `secret` for `lifetime`, filed under each of `owners`, unless one of
    /// them was revoked within [`REVOCATION_GRACE`]. Nothing is cached for a lifetime under a
    /// second. A failure is logged: the answer is only not cached.
    pub async fn put<Answer: Serialize>(
        &self,
        redis: &ConnectionManager,
        secret: &str,
        answer: &Answer,
        lifetime: Duration,
        owners: &[Owner],
    ) {
        let seconds = lifetime.as_secs();
        if seconds == 0 {
            return;
        }
        let serialized = serde_json::to_string(answer).expect("a cached answer serializes");

        let mut invocation = PUT.prepare_invoke();
        invocation.key(self.entry_key(secret));
        for owner in owners {
            invocation
                .key(self.owner_key(owner))
                .key(self.revoked_key(owner));
        }
        invocation.arg(serialized).arg(seconds);

        let mut redis = redis.clone();
        match invocation.invoke_async::<u8>(&mut redis).await {
            Ok(1) => {}
            Ok(_) => event!(
                name: "auth.cache.withheld",
                Level::DEBUG,
                cache.namespace = self.namespace,
                "left an answer uncached: its owner was just revoked",
            ),
            Err(error) => event!(
                name: "auth.cache.write_failure",
                Level::WARN,
                cache.namespace = self.namespace,
                error.message = %error,
                "could not cache an answer",
            ),
        }
    }

    /// Drops the answer cached for `secret`, so the next request asks again, such as after a
    /// session was extended. A failure is logged: the entry then lives out its lifetime.
    pub async fn remove(&self, redis: &ConnectionManager, secret: &str) {
        let mut redis = redis.clone();
        if let Err(error) = redis.del::<_, ()>(self.entry_key(secret)).await {
            event!(
                name: "auth.cache.remove_failure",
                Level::WARN,
                cache.namespace = self.namespace,
                error.message = %error,
                "could not drop a cached answer",
            );
        }
    }

    /// Drops every answer filed under `owner`, and keeps any from being cached for it for
    /// [`REVOCATION_GRACE`]. Call it once Kratos or Hydra has revoked what `owner` holds: the
    /// mark then covers every answer given before.
    ///
    /// A failure is logged as an error rather than returned. Kratos or Hydra already refuses what
    /// was revoked, and while Redis is unreachable nothing is read from it either; only entries a
    /// Redis that returns still holds could answer for it, until they expire.
    pub async fn revoke(&self, redis: &ConnectionManager, owner: &Owner) {
        match self.drop_owner(redis, owner).await {
            Ok(dropped) => event!(
                name: "auth.cache.revoked",
                Level::DEBUG,
                cache.namespace = self.namespace,
                cache.entries = dropped,
                "dropped {{cache.entries}} cached answers of a revoked owner",
            ),
            Err(error) => event!(
                name: "auth.cache.revoke_failure",
                Level::ERROR,
                cache.namespace = self.namespace,
                error.message = %error,
                "could not drop a revoked owner's cached answers; they answer until they expire",
            ),
        }
    }

    /// Marks `owner`, then deletes every entry its set names and the set itself. The mark comes
    /// first: an entry cached before it is in the set by then, and none can be cached after it.
    async fn drop_owner(&self, redis: &ConnectionManager, owner: &Owner) -> RedisResult<usize> {
        let mut redis = redis.clone();
        redis
            .set_ex::<_, _, ()>(self.revoked_key(owner), 1, REVOCATION_GRACE.as_secs())
            .await?;
        let owner_key = self.owner_key(owner);
        let mut keys: Vec<String> = redis.smembers(&owner_key).await?;
        let dropped = keys.len();
        keys.push(owner_key);
        redis.del::<_, ()>(keys).await?;
        Ok(dropped)
    }

    /// An entry's key. Hashed, so Redis never holds a usable secret.
    fn entry_key(&self, secret: &str) -> String {
        format!(
            "{}:entry:{}",
            self.namespace,
            hex::encode(Sha256::digest(secret))
        )
    }

    fn owner_key(&self, owner: &Owner) -> String {
        format!("{}:owner:{}", self.namespace, owner.0)
    }

    fn revoked_key(&self, owner: &Owner) -> String {
        format!("{}:revoked:{}", self.namespace, owner.0)
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn entry_keys_never_contain_the_secret() {
        let cache = RevocableCache::new("elysium:test");
        let key = cache.entry_key("ory_st_secret-token-value");
        assert!(key.starts_with("elysium:test:entry:"));
        assert!(!key.contains("secret-token-value"));
        assert_eq!(
            key,
            cache.entry_key("ory_st_secret-token-value"),
            "stable per secret"
        );
    }

    #[test]
    fn owners_of_different_kinds_never_share_keys() {
        let cache = RevocableCache::new("elysium:test");
        let id = Uuid::now_v7();
        assert_ne!(
            cache.owner_key(&Owner::new("subject", &id)),
            cache.owner_key(&Owner::new("client", &id)),
        );
    }

    /// The cache against a real Redis: filing, revoking, and the grace a revocation leaves. Every
    /// test names its own secrets and owners, so tests sharing one Redis never meet.
    mod store {
        use super::*;

        const CACHE: RevocableCache = RevocableCache::new("elysium:test:revocable");
        const LIFETIME: Duration = Duration::from_mins(5);

        async fn redis() -> ConnectionManager {
            let url = std::env::var("TEST_REDIS_URL")
                .expect("TEST_REDIS_URL must point at a disposable Redis");
            crate::connections::connect_redis(&secrecy::SecretString::from(url))
                .await
                .expect("TEST_REDIS_URL accepts connections")
        }

        /// A secret no other test uses.
        fn secret(name: &str) -> String {
            format!("{name}-{}", Uuid::now_v7().simple())
        }

        #[tokio::test]
        #[ignore = "needs TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
        async fn revoking_an_owner_drops_every_answer_filed_under_it() {
            let redis = redis().await;
            let (laptop, phone, desk) = (secret("laptop"), secret("phone"), secret("desk"));
            let person = Owner::new("identity", &Uuid::now_v7());
            let someone_else = Owner::new("identity", &Uuid::now_v7());
            CACHE
                .put(
                    &redis,
                    &laptop,
                    &"ada",
                    LIFETIME,
                    std::slice::from_ref(&person),
                )
                .await;
            CACHE
                .put(
                    &redis,
                    &phone,
                    &"ada",
                    LIFETIME,
                    std::slice::from_ref(&person),
                )
                .await;
            CACHE
                .put(&redis, &desk, &"grace", LIFETIME, &[someone_else])
                .await;

            CACHE.revoke(&redis, &person).await;

            assert_eq!(CACHE.get::<String>(&redis, &laptop).await, None);
            assert_eq!(CACHE.get::<String>(&redis, &phone).await, None);
            assert_eq!(
                CACHE.get::<String>(&redis, &desk).await.as_deref(),
                Some("grace"),
                "another owner's answers stay"
            );
        }

        /// The race the mark closes: an answer Kratos gave before the revocation, cached after it.
        #[tokio::test]
        #[ignore = "needs TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
        async fn an_answer_cached_after_its_owner_was_revoked_is_kept_out() {
            let redis = redis().await;
            let (token, other_token) = (secret("token"), secret("other token"));
            let subject = Owner::new("subject", &Uuid::now_v7());
            let client = Owner::new("client", &Uuid::now_v7());

            CACHE.revoke(&redis, &client).await;
            CACHE
                .put(
                    &redis,
                    &token,
                    &"answer",
                    LIFETIME,
                    &[subject.clone(), client],
                )
                .await;
            assert_eq!(
                CACHE.get::<String>(&redis, &token).await,
                None,
                "either owner's mark keeps it out"
            );

            CACHE
                .put(&redis, &other_token, &"answer", LIFETIME, &[subject])
                .await;
            assert_eq!(
                CACHE.get::<String>(&redis, &other_token).await.as_deref(),
                Some("answer"),
                "an owner nobody revoked is cached as usual"
            );
        }

        #[tokio::test]
        #[ignore = "needs TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
        async fn an_owner_set_lives_as_long_as_its_longest_entry() {
            let mut redis = redis().await;
            let person = Owner::new("identity", &Uuid::now_v7());
            CACHE
                .put(
                    &redis,
                    &secret("long"),
                    &1,
                    LIFETIME,
                    std::slice::from_ref(&person),
                )
                .await;
            CACHE
                .put(
                    &redis,
                    &secret("short"),
                    &2,
                    Duration::from_secs(5),
                    std::slice::from_ref(&person),
                )
                .await;

            let remaining: i64 = redis.ttl(CACHE.owner_key(&person)).await.expect("TTL");
            assert!(
                remaining > 5,
                "a shorter entry never shortens its owner's set: {remaining}s"
            );
        }

        #[tokio::test]
        #[ignore = "needs TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
        async fn an_answer_with_under_a_second_to_live_is_not_cached() {
            let redis = redis().await;
            let expiring = secret("expiring");
            CACHE
                .put(&redis, &expiring, &1, Duration::from_millis(900), &[])
                .await;
            assert_eq!(CACHE.get::<u8>(&redis, &expiring).await, None);
        }
    }
}
