# FIRMUPS Backend


## Prepare database
1. Enter dev-shell `nix develop`
2. Start postgres server `docker-compose -f ./db/docker-compose.yaml up -d`
2. Run migrations `diesel migration run`

## Update schema
1. Run migrations see "Prepare database"
2. Replace schema `diesel print-schema > src/db/schema.rs`