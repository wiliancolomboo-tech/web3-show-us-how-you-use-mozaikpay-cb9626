# Set Solana CLI to Devnet
solana config set --url https://api.devnet.solana.com

# Build program binary
anchor build

# Sync Program ID with generated keypair
anchor keys sync

# Deploy to Devnet
anchor deploy --provider.cluster devnet