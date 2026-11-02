# 1. Clone repository and install dependencies
git clone https://github.com/mozaikpay-enterprise/mozaikpulse.git
cd mozaikpulse
npm install

# 2. Build smart contract Anchor artifacts
anchor build

# 3. Synchronize program ID (Devnet target)
solana address -k target/deploy/mozaik_enterprise_pay-keypair.json
# Replace declare_id!() in lib.rs with output address
anchor build

# 4. Deploy Anchor Program to Solana Devnet
solana config set --url https://api.devnet.solana.com
solana airdrop 2
anchor deploy --provider.cluster devnet

# 5. Compile Client & CLI suite
npm run build