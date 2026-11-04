# 1. Clone & Install Dependencies
git clone https://github.com/mozaikpay/flowguard-orchestrator.git
cd flowguard-orchestrator
npm install

# 2. Build and Deploy Anchor Smart Contract to Solana Devnet / Mainnet
anchor build
solana program deploy target/deploy/mozaik_pay_escrow.so \
  --program-id target/deploy/mozaik_pay_escrow-keypair.json \
  --url devnet

# 3. Configure Production Environment
cp .env.example .env
# Edit .env with your RESEND_API_KEY, SLACK_WEBHOOK_URL, and SOLANA_RPC_URL

# 4. Run Full Anchor Test Suite
anchor test

# 5. Launch Turnkey Docker Service with Automated Healthchecks
docker-compose up -d --build
docker-compose ps