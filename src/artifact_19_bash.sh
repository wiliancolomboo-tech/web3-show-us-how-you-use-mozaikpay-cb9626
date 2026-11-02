# 1. Clone repository & install dependencies
git clone https://github.com/mozaikpay-enterprise/mozaikflow.git
cd mozaikflow

# 2. Build on-chain program via Anchor CLI
anchor build

# 3. Deploy to Solana Devnet
solana program deploy target/deploy/mozaikflow.so --program-id MozkF1ow11111111111111111111111111111111111

# 4. Configure environment
cp .env.example .env
# Edit .env with your RPC URL, Operator Private Key, and Slack/Resend tokens

# 5. Launch turnkey daemon via Docker Compose
docker compose up -d --build

# 6. Verify service health and active polling
curl http://localhost:8080/healthz
curl http://localhost:8080/metrics