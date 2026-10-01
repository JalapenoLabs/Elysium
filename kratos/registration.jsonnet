// The body Kratos posts to Elysium before it stores a new identity signing up with a
// password. Elysium answers whether the sign-up may proceed. See docs/auth.md.
function(ctx) {
  method: 'password',
  email: ctx.identity.traits.email,
}
