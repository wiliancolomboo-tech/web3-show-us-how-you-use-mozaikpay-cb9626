import { EmailNotificationService } from '../services/emailService';
import { SolanaSettlementListener } from '../services/solanaListener';

describe('MozaikEngine Enterprise QA Suite', () => {
  let emailService: EmailNotificationService;
  let listener: SolanaSettlementListener;

  beforeAll(() => {
    process.env.APP_ENV = 'test';
    emailService = new EmailNotificationService();
    listener = new SolanaSettlementListener();
  });

  test('Deterministic Split Calculations: 85% Merchant, 10% Affiliate, 5% Reserve', () => {
    const grossAmount = 1_000_000_000; // 1,000 USDC with 6 decimals
    const affiliateBps = 1000; // 10%
    const reserveBps = 500;    // 5%

    const affiliateShare = (grossAmount * affiliateBps) / 10_000;
    const reserveShare = (grossAmount * reserveBps) / 10_000;
    const merchantShare = grossAmount - (affiliateShare + reserveShare);

    expect(affiliateShare).toBe(100_000_000); // 100 USDC
    expect(reserveShare).toBe(50_000_000);    // 50 USDC
    expect(merchantShare).toBe(850_000_000);  // 850 USDC
    expect(merchantShare + affiliateShare + reserveShare).toBe(grossAmount);
  });

  test('Email Dispatch Service delivers receipt in test mode', async () => {
    const result = await emailService.sendInvoiceReceipt({
      recipientEmail: 'enterprise_buyer@acme.com',
      customerPubkey: 'DemoCustomer1111111111111111111111111111111',
      invoiceIdHex: 'e3b0c44298fc1c149afbf4c8996fb924',
      amountUsdc: 99.00,
      signature: '5K2bW78MhVwUjH9J1R8uFkS3vL3mQ4xY1zP0aB2cD4eF6gH8iJ0kL2mN4oP6qR8s',
      timestamp: new Date().toISOString()
    });

    expect(result).toBe(true);
  });

  test('Pending Invoice Registration and State Retrieval', () => {
    const mockRef = '4uQeVj5tqViQh7yWWGStvkEG1Zmhx6uasJtWCJziofM';
    listener.registerPendingInvoice({
      invoiceIdHex: 'aabbcc112233',
      referencePubkey: mockRef,
      expectedAmount: 50.00,
      recipientEmail: 'client@partner.com',
      status: 'PENDING'
    });

    // Verification when no Tx has landed should return false gracefully
    return listener.pollReferenceAccount(mockRef).then((settled) => {
      expect(settled).toBe(false);
    });
  });
});