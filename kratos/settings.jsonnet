// The body Kratos posts to Elysium once a person's settings change is stored, so Elysium forgets
// the sessions it cached for them and each asks Kratos again. See docs/auth.md.
function(ctx) {
  identity_id: ctx.identity.id,
}
