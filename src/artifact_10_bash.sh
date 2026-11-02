# Configure Solana CLI for Devnet
solana config set --url https://api.devnet.solana.com

# Fund deployer wallet
solana airdrop 2

# Build and sync program keypair
anchor build
solana address -k target/deploy/mozaik_settle-keypair.json

# Deploy program
anchor deploy --program-name mozaik_settle