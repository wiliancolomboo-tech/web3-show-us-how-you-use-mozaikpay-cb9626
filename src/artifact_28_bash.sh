# Pause on-chain settlement
anchor run set-paused -- --paused true

# Stop external side effects while retaining API visibility
docker compose stop worker

# Keep API online for status and support
curl https://relay.example.com/healthz