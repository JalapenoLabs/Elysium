// The body Kratos posts to Elysium for every message it would email. With no mail
// server, Elysium logs recovery links for an operator to pass on. See docs/auth.md.
function(ctx) {
  recipient: ctx.recipient,
  template_type: ctx.template_type,
  recovery_url: if 'recovery_url' in ctx.template_data then ctx.template_data.recovery_url else null,
  expires_in_minutes: if 'expires_in_minutes' in ctx.template_data then ctx.template_data.expires_in_minutes else null,
}
