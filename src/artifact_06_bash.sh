# 1. Clone repository & install dependencies
git clone https://github.com/mozaikflow/mozaik-checkout-engine.git
cd mozaik-checkout-engine
npm install

# 2. Build on-chain program
anchor build

# 3. Deploy to Solana Devnet
solana config set --url https://api.devnet.solana.com
solana airdrop 2
anchor deploy

# 4. Copy Program ID and update Anchor.toml & lib.rs declare_id!
# Then run full test suite:
anchor test