// The body Kratos posts to Elysium before it stores a new identity signing up with a
// passkey. Elysium refuses it: every account starts with a password. See docs/auth.md.
function(ctx) {
  method: 'passkey',
  email: ctx.identity.traits.email,
}
