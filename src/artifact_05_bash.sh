# 1. Clone repository & install dependencies
git clone https://github.com/mozaikflow/mozaikflow-enterprise.git
cd mozaikflow-enterprise
npm install

# 2. Build Anchor Smart Contracts
anchor build

# 3. Synchronize program keys & deploy to Solana Devnet
solana config set --url https://api.devnet.solana.com
anchor deploy --provider.cluster devnet

# 4. Run full deterministic test suite
anchor test