import axios from 'axios';

export class SlackService {
  constructor(private webhookUrl: string) {}

  async notifyPaymentSettled(
    orderId: string,
    customer: string,
    amountUsdc: string,
    tier: string,
    txSignature: string
  ): Promise<void> {
    const payload = {
      text: `🚀 *MozaikPay Payment Verified & Split Executed*`,
      blocks: [
        {
          type: "header",
          text: {
            type: "plain_text",
            text: "💰 Revenue Received via MozaikPay",
            emoji: true
          }
        },
        {
          type: "section",
          fields: [
            { type: "mrkdwn", text: `*Order ID:*\n\`${orderId}\`` },
            { type: "mrkdwn", text: `*Amount:*\n*${amountUsdc} USDC*` },
            { type: "mrkdwn", text: `*Tier:*\n${tier}` },
            { type: "mrkdwn", text: `*Customer:*\n\`${customer.substring(0, 4)}...${customer.substring(customer.length - 4)}\`` }
          ]
        },
        {
          type: "context",
          elements: [
            {
              type: "mrkdwn",
              text: `🔗 <https://solscan.io/tx/${txSignature}|View On-Chain Settlement>`
            }
          ]
        }
      ]
    };

    await axios.post(this.webhookUrl, payload, { timeout: 5000 });
  }
}