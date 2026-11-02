# 1. Clone repository & install dependencies
git clone https://github.com/mozaikpay/mozaikpay-nexus.git
cd mozaikpay-nexus
npm install

# 2. Build Solana Anchor Program
anchor build

# 3. Deploy to Solana Devnet / Mainnet
anchor deploy --provider.cluster devnet

# 4. Run full test suite with 100% assertions
anchor test

# 5. Launch Enterprise Relay Daemon via Docker
docker compose up -d --build

# 6. Verify healthcheck telemetry
curl -f http://localhost:8080/health