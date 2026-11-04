# Configure Solana CLI to devnet
solana config set --url https://api.devnet.solana.com

# Build the Anchor program
anchor build

# Sync keys and obtain program ID
anchor keys sync

# Deploy to Solana Devnet
anchor deploy --provider.cluster devnet