docker compose pull
docker compose run --rm api npm run migrate
docker compose up -d --no-deps api worker
curl -f https://relay.example.com/readyz