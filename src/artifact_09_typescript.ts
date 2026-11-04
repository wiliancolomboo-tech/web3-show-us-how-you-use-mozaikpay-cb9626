import express from "express";

const app = express();
const port = process.env.PORT || 8080;

app.get("/health", (req, res) => {
  res.status(200).json({
    status: "healthy",
    uptime: process.uptime(),
    timestamp: new Date().toISOString(),
    network: "solana-devnet",
    protocol: "MozaikPay-Engine v1.0.0",
  });
});

app.listen(port, () => {
  console.log(`[HEALTH] Health check listening on port ${port}`);
});