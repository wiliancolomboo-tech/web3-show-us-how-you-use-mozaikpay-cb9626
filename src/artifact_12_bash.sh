curl -X POST http://localhost:3000/api/checkout \
  -H "Content-Type: application/json" \
  -d '{
    "orderId": "PO-SMOKE-2025",
    "amountUsdc": 25.00,
    "customerEmail": "lead_architect@enterprise.io",
    "customerName": "Sponsor Judge"
  }'