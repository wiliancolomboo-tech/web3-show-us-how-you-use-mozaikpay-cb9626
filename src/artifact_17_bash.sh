curl -X POST http://localhost:8080/api/v1/invoices \
  -H "Content-Type: application/json" \
  -d '{
    "amountUsdc": 49.99,
    "customerEmail": "cto@enterprise-partner.io",
    "label": "MozaikPay Premium Subscription",
    "memo": "Sub-Tier-Enterprise-Q1"
  }'