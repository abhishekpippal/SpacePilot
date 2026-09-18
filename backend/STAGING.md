# SpacePilot staging deployment

## Architecture and platform

Use one Render Docker web service and one Render managed PostgreSQL database. The native development desktop calls `https://staging-api.spacepilot.app`; OpenAI, Resend, Stripe and signing keys exist only in Render runtime environment variables. Render is recommended for the first staging environment because the repository already uses Docker and Render provides managed PostgreSQL, secret environment variables, logs, health checks, custom domains and managed HTTPS without Kubernetes.

## Environment boundary

Set `APP_ENV=staging`. Create unique staging values for every variable listed in `.env.example`; never copy production secrets. Mandatory staging security values are `DATABASE_URL`, a 32+ character `JWT_SIGNING_SECRET`, a base64url-encoded 32-byte `ENTITLEMENT_SIGNING_PRIVATE_KEY`, HTTPS return/reset URLs, `TRUSTED_HOSTS=staging-api.spacepilot.app,<render-host>.onrender.com`, and Render's proxy range in `FORWARDED_ALLOW_IPS`.

Stripe is optional until sandbox work begins. If any Stripe value is set, all four test values must be set: `STRIPE_SECRET_KEY` beginning `sk_test_`, `STRIPE_WEBHOOK_SECRET`, `STRIPE_PRICE_PRO_MONTHLY`, and `STRIPE_PRICE_PRO_ANNUAL`. Missing billing configuration causes billing endpoints to fail; it never creates fake success. Resend and OpenAI keys are optional for initial infrastructure validation, but email/AI end-to-end acceptance remains incomplete until configured. Staging responses never return reset or verification tokens.

Browser CORS is unnecessary for Rust-mediated Tauri API calls. Leave `CORS_ALLOWED_ORIGINS` empty unless an owned staging web page calls the API; then list exact HTTPS origins. Never use `*` with credentials.

## Render procedure

1. Create a Render account/project named `SpacePilot Staging` and connect the repository.
2. Apply the root `render.yaml`, or create equivalent Docker web service and PostgreSQL resources.
3. Select a paid PostgreSQL plan if backup/PITR validation is required. Use the internal/private database URL and do not grant the application role superuser privileges.
4. Enter staging secrets in Render; do not place them in Blueprint source or Docker build arguments.
5. Before the first API deploy, run `python -m alembic upgrade head` as Render's pre-deploy command or a controlled one-off job. Never run `downgrade` against staging automatically.
6. Deploy and confirm `/health` and `/ready` return HTTP 200 on the Render hostname.
7. Add `staging-api.spacepilot.app` as a custom domain. At the DNS provider create the exact CNAME Render displays, then verify the domain and HTTPS certificate in Render.
8. In a local debug desktop session set `SPACEPILOT_BACKEND_URL=https://staging-api.spacepilot.app`. Release builds ignore this override and retain `https://api.spacepilot.app`.

## Migration, rollback and recovery

Forward staging migration: create/verify a backup, deploy code compatible with the current schema, run `alembic current`, run `alembic upgrade head`, then validate readiness. Application rollback means redeploying the prior compatible image; do not downgrade the database unless a reviewed recovery plan explicitly requires it. For data loss, restore Render PITR into a separate database, validate it, then change `DATABASE_URL`. Periodically create a logical export and perform an isolated restore test. Backups are not considered verified until a restore succeeds.

Rotate JWT/payment/email/OpenAI credentials independently in Render and restart the service. Entitlement signing-key rotation requires a desktop public-key transition because old offline assertions use the prior public key.

## Provider acceptance

Stripe: create sandbox products/prices, configure the signed webhook at `/v1/billing/webhook`, and perform a test-mode subscription lifecycle. Resend: verify a staging sending domain and send to a controlled recipient. OpenAI: set a staging project key with provider budget limits and perform one minimized-context request. Never use live payment credentials in staging.
