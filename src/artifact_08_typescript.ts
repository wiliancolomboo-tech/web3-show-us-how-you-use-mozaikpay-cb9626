import { Command } from "commander";
import axios from "axios";

const program = new Command();
program.name("mozaikflow-cli").description("MozaikFlow Enterprise Management CLI").version("1.0.0");

program
  .command("create-session")
  .description("Create a new MozaikPay subscription invoice")
  .requiredOption("-e, --email <email>", "Customer email address")
  .requiredOption("-p, --plan <plan>", "Product/Plan name")
  .requiredOption("-a, --amount <amount>", "Amount in USDC", parseFloat)
  .requiredOption("-m, --merchant-vault <vault>", "Merchant USDC ATA")
  .requiredOption("-f, --affiliate-vault <vault>", "Affiliate USDC ATA")
  .requiredOption("-t, --treasury-vault <vault>", "Treasury USDC ATA")
  .action(async (opts) => {
    try {
      const res = await axios.post("http://localhost:4000/api/checkout/create-invoice", {
        customerEmail: opts.email,
        planName: opts.plan,
        amountUSDC: opts.amount,
        merchantVault: opts.merchantVault,
        affiliateVault: opts.affiliateVault,
        treasuryVault: opts.treasuryVault,
      });

      console.log("\n================ MOZAIKPAY CHECKOUT SESSION CREATED ================");
      console.log(`Invoice ID:  ${res.data.data.invoiceId}`);
      console.log(`Reference:   ${res.data.data.reference}`);
      console.log(`Solana Pay:  ${res.data.data.paymentUrl}`);
      console.log("====================================================================\n");
    } catch (err: any) {
      console.error("Failed to create session:", err.response?.data || err.message);
    }
  });

program
  .command("inspect")
  .description("Query invoice state by ID")
  .argument("<invoiceId>", "32-byte hexadecimal invoice ID")
  .action(async (invoiceId) => {
    try {
      const res = await axios.get(`http://localhost:4000/api/checkout/invoice/${invoiceId}`);
      console.log("\nInvoice State:", JSON.stringify(res.data.data, null, 2));
    } catch (err: any) {
      console.error("Failed to query invoice:", err.response?.data || err.message);
    }
  });

program.parse(process.argv);