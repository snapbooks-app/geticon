# GetIcon

## Dependencies: 14-day grace period

Never add or upgrade to a package version published less than 14 days ago. This covers crates in `Cargo.lock`, Docker base images and GitHub Actions.

Cargo has no minimum-age setting, so check it yourself after any `cargo update`, `cargo add` or other lockfile change:

- A crate version's publish date is `version.created_at` from `https://crates.io/api/v1/crates/<name>/<version>`.
- Pin a version that is too new back to the newest older release in the same semver range with `cargo update -p <name>@<version> --precise <older>`, then check again, since a downgrade can pull in other versions.

## Docker image

- The runtime image must include `curl`: the deploy (`backend/scripts/deploy.sh` in `snapbooks-app/mono`) runs `curl -f http://localhost:8080/health` as the container health check and waits until the container is healthy.
- The Docker workflow only publishes when the `Cargo.toml` version is not yet in the registry, so bump the version (and `CHANGELOG.md`) for every release.
