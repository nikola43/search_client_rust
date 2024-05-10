import { Connection, Keypair, LAMPORTS_PER_SOL, PublicKey, SystemProgram, Transaction } from "@solana/web3.js";
import bs58 from "bs58";
import { load, DataType, open, close } from 'ffi-rs';
import 'dotenv/config'
import os from 'node:os'





async function main() {
  const PRIVATE_KEY = process.env.PRIVATE_KEY!;
  const platform = os.platform();
  let dynamicLib = "./libsend_bundle_rs.dylib";
  // switch (platform) {
  //   case 'darwin':
  //     dynamicLib = "./target/aarch64-apple-darwin/release/libtpu_client.dylib"
  //     break;
  //   case 'linux':
  //     dynamicLib = "./target/release/libtpu_client.so"
  //     break;
  //   case 'win32':
  //     dynamicLib = "./target/release/libtpu_client.dll"
  //     break;
  //   default:
  //     throw new Error("Unsupported platform");
  // }

  //const rpc_url = "https://api.devnet.solana.com";
  //const ws_url = "wss://api.devnet.solana.com";

  const rpc_url = "https://api.mainnet-beta.solana.com"
  const ws_url = "wss://api.mainnet-beta.solana.com"
  const connection = new Connection(rpc_url, {
    wsEndpoint: ws_url,
  });

  open({
    library: 'libsend_bundle_rs', // key
    path: dynamicLib
  })

  const wallet = Keypair.fromSecretKey(bs58.decode(PRIVATE_KEY));
  const recipient = new PublicKey("GKxpQ3ZSMNSbCDs1RrqhxuTTUfn8xx6faDzTss38mkw3")
  const amountLamports = LAMPORTS_PER_SOL / 100; // Sending 0.01 SOL

  let jito_pubkey = new PublicKey("HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe");
  const jito_tip = 1500000

  const transaction = new Transaction();
  const latestBlockhash = await connection.getLatestBlockhash({
    commitment: "finalized",
  });
  transaction.recentBlockhash = latestBlockhash.blockhash;
  transaction.add(
    SystemProgram.transfer({
      fromPubkey: wallet.publicKey,
      toPubkey: recipient,
      lamports: amountLamports,
    })
  );
  transaction.add(
    SystemProgram.transfer({
      fromPubkey: wallet.publicKey,
      toPubkey: jito_pubkey,
      lamports: jito_tip,
    })
  );
  transaction.sign(wallet);

  /*
    block_engine_url: *const i8,
    auth_keypair_path: *const i8,
    rpc_ws_url: *const i8,
    wire_transaction: *const u8, // Vec<u8>
    wire_transaction_len: usize, // Length of the transaction data

  */

  let block_engine_url = "https://amsterdam.mainnet.block-engine.jito.wtf";
  let auth_keypair_path = "./jito-keypair.json";

  // Send the transaction to the network
  const serializedTransaction = transaction.serialize();
  const signature = bs58.encode(transaction.signature!);
  console.log("Transaction sent:", signature);
  const txResult = load({
    library: "libsend_bundle_rs", // path to the dynamic library file
    funcName: 'send_bundle_with_confirmation', // the name of the function to call
    retType: DataType.Boolean, // the return value type
    paramsType: [DataType.String, DataType.String, DataType.String, DataType.U8Array, DataType.I32], // the parameter types
    paramsValue: [block_engine_url, auth_keypair_path, ws_url, serializedTransaction, serializedTransaction.length] // the actual parameter values
  })
  close('libsend_bundle_rs')

  console.log("Transaction sent:", txResult);

  // compute tx signature
  // const signature = bs58.encode(transaction.signature!);
  console.log("Transaction sent:", signature);
  if (txResult) {
    console.log(`https://explorer.solana.com/tx/${signature}`);
  }
}

main().catch((error) => {
  console.error("Error:", error);
});
