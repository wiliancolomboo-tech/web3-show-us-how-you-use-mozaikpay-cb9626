# 1. Clone & Enter Project Repository
git clone https://github.com/mozaikflow/mozaikpay-enterprise.git
cd mozaikpay-enterprise

# 2. Install Dependencies & Build Anchor Artifacts
npm install
anchor build

# 3. Deploy Smart Contract to Solana Devnet
solana config set --url https://api.devnet.solana.com
solana airdrop 2
anchor deploy --provider.cluster devnet

# 4. Run Full Verification Test Suite
npm test