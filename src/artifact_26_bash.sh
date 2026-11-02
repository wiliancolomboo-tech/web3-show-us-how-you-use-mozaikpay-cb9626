# Launch the turnkey service
docker-compose up -d --build

# Verify health status
curl -i http://localhost:8080/healthz