# 1. Clone repository & install dependencies
git clone https://github.com/mozaikflow/mozaikflow-core.git
cd mozaikflow-core
yarn install

# 2. Build Anchor Smart Contracts
anchor build

# 3. Configure environment variables for devnet or mainnet
cp .env.example .env
# Edit .env with SOLANA_RPC_URL, RESEND_API_KEY, SLACK_WEBHOOK_URL

# 4. Deploy Anchor Program to Solana Devnet
solana config set --url https://api.devnet.solana.com
anchor deploy --provider.cluster devnet

# 5. Run end-to-end integration test suite
anchor test