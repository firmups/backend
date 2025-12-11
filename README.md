# FIRMUPS Backend


## Prepare database
1. Enter dev-shell `nix develop`
2. Start postgres server `docker-compose -f ./db/docker-compose.yaml up -d`
2. Run migrations `diesel migration run`
