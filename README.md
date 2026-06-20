# FIRMUPS Backend

## Create production build

1. Run nix build `nix build .#backend`
2. The resulting prod file is symlinked to `./result`

## Create docker image

1. Run nix build `nix build .#dockerImage`
2. The resulting docker image tarball file is symlinked to `./result`
3. Load the created image with `docker load < ./result`

## Development setup

1. Enter dev-shell `nix develop`
2. Install cargo dependencies `cargo install`
3. Start Postgres server `docker compose -f ./db/docker-compose.yaml up -d`
4. Run migrations `diesel migration run`

docker compose up storage
docker exec -it backend-storage-1 /garage status
docker exec -it backend-storage-1 /garage layout assign -z dc1 -c 1G <node_id>
docker exec -it backend-storage-1 /garage layout apply --version 1
docker exec -it backend-storage-1 /garage bucket create firmups-bucket

# Create key and get it

output=$(docker exec backend-storage-1 /garage key create firmups-app-key)
ACCESS_KEY=$(echo "$output" | sed -n 's/^Key ID:[ ]*//p')
SECRET_KEY=$(echo "$output" | sed -n 's/^Secret key:[ ]*//p')
echo "ACCESS_KEY=$ACCESS_KEY"
echo "SECRET_KEY=$SECRET_KEY"

# ToDo: Write to env:

docker exec -it backend-storage-1 /garage bucket allow --read --write --owner firmups-bucket --key firmups-app-key
