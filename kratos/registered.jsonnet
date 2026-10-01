// The body Kratos posts to Elysium once it has stored a new identity, so Elysium creates the
// person's account in the order people signed up. See docs/auth.md.
function(ctx) {
  identity_id: ctx.identity.id,
  email: ctx.identity.traits.email,
  name: ctx.identity.traits.name,
}
