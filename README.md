# FIRMUPS Backend


## Development setup
1. Enter dev-shell `nix develop`
2. Install cargo dependencies `cargo install`
3. Start Postgres server `docker compose -f ./db/docker-compose.yaml up -d`
4. Run migrations `diesel migration run`
