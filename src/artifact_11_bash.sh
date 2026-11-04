# Copy and configure enterprise environment keys
cp .env.example .env
nano .env

# Spin up daemon via Docker Compose
docker compose up -d --build

# Monitor live event logs
docker compose logs -f mozaik-pulse-daemon