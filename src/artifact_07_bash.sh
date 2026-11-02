# 1. Clone repository and install dependencies
git clone https://github.com/mozaikflow/mozaikflow-enterprise.git
cd mozaikflow-enterprise
npm install

# 2. Build and verify Anchor smart contract
anchor build
anchor test

# 3. Deploy to Solana Devnet
solana config set --url https://api.devnet.solana.com
solana airdrop 2
anchor deploy --provider.cluster devnet

# 4. Initialize Local Environment Variables
cp .env.example .env
# Edit .env with your RESEND_API_KEY and SLACK_WEBHOOK_URL

# 5. Launch the Enterprise Listener Daemon
npm run start:daemon