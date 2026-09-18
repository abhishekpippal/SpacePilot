# SpacePilot production operations

The API is designed for a small managed container service (Render or Railway) plus managed PostgreSQL. Production must set `APP_ENV=production`; startup rejects missing JWT, Stripe, Resend, or OpenAI secrets. Store all secrets in the hosting platform, never in an image or desktop build. Use separate Stripe test/live price IDs and webhook secrets per environment.

## Deployment gate

Create a managed PostgreSQL database with TLS, private networking where available, automated daily backups and provider point-in-time recovery. Run `alembic upgrade head` as a release command before starting the API. Configure `https://api.spacepilot.app/v1/billing/webhook` in Stripe and restrict CORS to explicitly owned web origins; native clients do not require browser CORS.

Verify a restore quarterly into an isolated database. Never describe backups as working until a restore has succeeded. Before destructive migrations, snapshot the database and document the application rollback compatible with the previous schema. Rotation of the entitlement private key invalidates cached assertions, so retain the previous public verification key during a controlled desktop transition.

## Windows signing and updates

Obtain an organization-validated or EV Authenticode certificate. Keep its private key in CI secret storage or a hardware/cloud signing service. Sign the executable and MSI with `signtool sign /fd SHA256 /tr https://timestamp.digicert.com /td SHA256`, then verify with `signtool verify /pa`. Tauri updater artifacts and update metadata must have a separate protected updater signing key and HTTPS endpoint. Do not enable the updater until signed artifacts and key rotation/revocation procedures are operational.

## Commercial readiness boundary

Stripe is the current adapter, not an entitlement dependency. A future Paddle adapter must implement checkout, customer portal, webhook verification, subscription lookup/cancellation, and normalize customer/subscription references, status, billing period, and cancellation. Internal plans and `reconcile_entitlement` remain unchanged. Fake providers are injected by tests and are never selected by production configuration.

Still external and unverified: Stripe credentials, sandbox products/prices, signed sandbox events and final refund policy; Resend domain/API key and delivery; staging hosting, managed PostgreSQL, DNS/HTTPS, verified restores and monitoring; Authenticode certificate, updater keys/endpoint, and clean-machine installer testing.

AI quotas reset by calendar month on the server. Billing grace defaults to three days after a past-due paid-period boundary. Offline assertion grace is a separate device-validation policy.

## Dependency advisories

The 2026-09-15 npm audit reports five development-tool findings: direct Vite (high) and Vitest (critical), plus transitive esbuild, vite-node, and `@vitest/mocker`. They affect development/test servers and are not packaged runtime dependencies. npm offers only major upgrades (Vite 8 and Vitest 5), so a dedicated compatibility upgrade is required; no force fix was applied.
