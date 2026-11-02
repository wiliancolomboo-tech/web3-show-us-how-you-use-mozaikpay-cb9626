cp .env.example .env
docker compose up --build
curl http://localhost:8080/healthz