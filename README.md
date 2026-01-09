# FIRMUPS Backend

## Create production build

1. Run nix build `nix build .#backend`
2. The resulting prod file is symlinked to `./result`

## Create docker image

1. Run nix build `nix build .#dockerImage`
2. The resulting docker image tarball file is symlinked to `./result`

## Development setup

1. Enter dev-shell `nix develop`
2. Install cargo dependencies `cargo install`
3. Start Postgres server `docker compose -f ./db/docker-compose.yaml up -d`
4. Run migrations `diesel migration run`
