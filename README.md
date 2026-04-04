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

sudo docker exec -it backend-garage-1 /garage status
sudo docker exec -it backend-garage-1 /garage layout assign -z dc1 -c 1G <node_id>
sudo docker exec -it backend-garage-1 /garage layout apply --version 1
sudo docker exec -it backend-garage-1 /garage bucket create firmups-bucket

# Create key and get it
output=$(sudo docker exec backend-garage-1 /garage key create firmups-app-key)
ACCESS_KEY=$(echo "$output" | sed -n 's/^Key ID:[ ]*//p')
SECRET_KEY=$(echo "$output" | sed -n 's/^Secret key:[ ]*//p')
echo "ACCESS_KEY=$ACCESS_KEY"
echo "SECRET_KEY=$SECRET_KEY"

# ToDo: Write to env:

sudo docker exec -it backend-garage-1 /garage bucket allow --read --write --owner firmups-bucket --key firmups-app-key
