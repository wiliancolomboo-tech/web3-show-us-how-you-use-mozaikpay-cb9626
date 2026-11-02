# 1. Build and verify deterministic Anchor artifact
anchor build

# 2. Deploy binary to Devnet
solana program deploy \
  --program-id target/deploy/mozaik_flow-keypair.json \
  target/deploy/mozaik_flow.so \
  --url-type devnet

# 3. Initialize Merchant Settlement Config via CLI
npm --prefix client run crank