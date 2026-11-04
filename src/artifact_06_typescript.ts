import { Command } from "commander";
import { Connection, Keypair, PublicKey } from "@solana/web3.js";
import { MozaikPulseOrchestrator } from "./orchestrator";
import * as fs from "fs";

const program = new Command();

program
  .name("mozaik-pulse-cli")
  .description("Enterprise management CLI for MozaikPay streams and checkout splits")
  .version("1.0.0");

program
  .command("start-daemon")
  .description("Run the background listener for Slack and Resend webhooks")
  .action(async () => {
    const orchestrator = new MozaikPulseOrchestrator();
    await orchestrator.startDaemon();
  });

program
  .command("simulate-payment")
  .description("Simulate an enterprise multi-vendor checkout split transaction")
  .requiredOption("-o, --order <id>", "Merchant Order Reference ID")
  .requiredOption("-a, --amount <amount>", "Amount in token micro-units (e.g. 100000000)")
  .action(async (options) => {
    console.log(`[SIMULATION] Triggering settlement for Order: ${options.order}, Amount: ${options.amount}`);
    const orchestrator = new MozaikPulseOrchestrator();
    await orchestrator.notifySlack("TEST Checkout Payment Triggered", {
      Order: options.order,
      Amount: options.amount,
      Status: "Simulated Success",
    });
    console.log("[SIMULATION COMPLETE] Check Slack and event logs.");
  });

program.parse(process.argv);