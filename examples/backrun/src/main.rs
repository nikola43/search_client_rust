use std::{
    panic,
    panic::PanicInfo,
    process, 
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use env_logger::TimestampPrecision;
use jito_protos::{
    auth::auth_service_client::AuthServiceClient, bundle::rejected::Reason, searcher::searcher_service_client::SearcherServiceClient
};
use jito_searcher_client::{
    client_interceptor::ClientInterceptor, cluster_data_impl::ClusterDataImpl, grpc_connect,
    SearcherClient,
    SearcherClientResult,
};

use solana_client::nonblocking::rpc_client::RpcClient;

use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    system_instruction::{self, transfer},
    transaction::Transaction,
};

use tonic::{codegen::InterceptedService, transport::Channel};
use jito_protos::bundle::bundle_result::Result;

#[tokio::main]
async fn main() {
    env_logger::builder()
        .format_timestamp(Some(TimestampPrecision::Micros))
        .init();

    let lamports = 1000;
    let rpc_url = "https://api.mainnet-beta.solana.com";
    let ws_url = "wss://api.mainnet-beta.solana.com";
    let block_engine_url = "https://amsterdam.mainnet.block-engine.jito.wtf";
    let auth_keypair_path = "./jito-keypair.json";
    let auth_keypair =
        Arc::new(read_keypair_file(auth_keypair_path).expect("reads keypair at path"));
    let payer = "./my-keypair.json";
    let payer_keypair = read_keypair_file(&payer).expect("reads keypair at path");
    let exit = Arc::new(AtomicBool::new(false));
    let recipient_pubkey =
        Pubkey::from_str("GKxpQ3ZSMNSbCDs1RrqhxuTTUfn8xx6faDzTss38mkw3").unwrap();
    let lamports_to_send = 1_000; // 1 SOL

    let rpc_client =
        RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());



    // build + sign the transactions
    let blockhash = rpc_client
        .get_latest_blockhash()
        .await
        .expect("get blockhash");

    let transfer_instruction = system_instruction::transfer(
        &payer_keypair.pubkey(), // From (payer)
        &recipient_pubkey,       // To (recipient)
        lamports_to_send,        // Amount
    );

    let tip_account =
    Pubkey::from_str("HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe").unwrap();

    let jito_tip_instruction = transfer(&payer_keypair.pubkey(), &tip_account, lamports);

    let mut transaction = Transaction::new_with_payer(
        &[transfer_instruction, jito_tip_instruction],
        Some(&payer_keypair.pubkey()),
    );
    transaction.sign(&[&payer_keypair], blockhash);
    //let wire_transaction = bincode::serialize(&transaction).unwrap();
    //let transaction_signature = transaction.signatures[0];

    //let txs: &Vec<Signature> = &vec![transaction.signatures[0]];
    let wire_transaction = bincode::serialize(&transaction).unwrap();
    let vec_of_vecs: Vec<Vec<u8>> = vec![wire_transaction];
    let txs: &[Vec<u8>] = &vec_of_vecs;
    let singature = transaction.signatures[0];
    println!("singature: {:?}", singature);

    let tx_accepted =
        send_bundle_with_confirmation(auth_keypair_path, block_engine_url, ws_url, txs).await;
        println!("tx_accepted: {:?}", tx_accepted);
}

async fn send_bundle_with_confirmation(
    auth_keypair_path: &str,
    block_engine_url: &str,
    rpc_url: &str,
    txs: &[Vec<u8>],
) -> bool {
    let mut tx_accepted = false;
    let auth_keypair =
        Arc::new(read_keypair_file(auth_keypair_path).expect("reads keypair at path"));
    // let exit = Arc::new(AtomicBool::new(false));

    let exit = graceful_panic(None);

    let searcher_client_service =
        get_searcher_client(&auth_keypair, &exit, block_engine_url, rpc_url)
            .await
            .unwrap();
    let (mut searcher_client, cluster_data) = searcher_client_service;

    let bundleId = searcher_client.send_bundle(txs).await.unwrap();
    println!("bundleId: {:?}", bundleId);

    let mut receiver_result = searcher_client
        .subscribe_bundle_results(1000000000)
        .await
        .unwrap();
    //let bundle_result = receiver_result.recv().await.unwrap();
    let bundle_result = receiver_result.recv().await;
    println!("bundle_result: {:?}", bundle_result);

    let bundle_result = receiver_result.recv().await.unwrap();
    let result = bundle_result.clone().result.unwrap();

    match result {
        Result::Accepted(accepted) => {
            println!("accepted: {:?}", accepted);
            tx_accepted = true;
        }
        Result::Rejected(rejected) => {
            println!("rejected: {:?}", rejected);

            // let reason = rejected.reason.unwrap();
            // println!("Error code 1: {:?}", reason);

            if let Some(Reason::SimulationFailure(simulation_failure)) = rejected.reason {
                println!("Simulation failure: {:?}", simulation_failure);

                if simulation_failure.msg.is_some() {
                    let msg = simulation_failure.msg.unwrap();
                    if msg == "This transaction has already been processed" {
                        tx_accepted = true;
                    }
                }
            }

            // if (reason) {
            //     println!("Error code 1: {:?}", rejected.error_message);
            // }
        }
        Result::Finalized(finalized) => {
            println!("finalized: {:?}", finalized);
            tx_accepted = true;
        }
        Result::Processed(processed) => {
            println!("processed: {:?}", processed);
        }
        Result::Dropped(dropped) => {
            println!("dropped: {:?}", dropped);
        }
    }

    return tx_accepted;
}

pub fn graceful_panic(callback: Option<fn(&PanicInfo)>) -> Arc<AtomicBool> {
    let exit = Arc::new(AtomicBool::new(false));
    // Fail fast!
    let panic_hook = panic::take_hook();
    {
        let exit = exit.clone();
        panic::set_hook(Box::new(move |panic_info| {
            if let Some(f) = callback {
                f(panic_info);
            }
            exit.store(true, Ordering::Relaxed);
            println!("exiting process");
            // let other loops finish up
            std::thread::sleep(Duration::from_secs(5));
            // invoke the default handler and exit the process
            panic_hook(panic_info); // print the panic backtrace. default exit code is 101

            process::exit(1); // bail us out if thread blocks/refuses to join main thread
        }));
    }
    exit
}


async fn get_searcher_client(
    auth_keypair: &Arc<Keypair>,
    exit: &Arc<AtomicBool>,
    block_engine_url: &str,
    rpc_pubsub_addr: &str,
) -> SearcherClientResult<(
    SearcherClient<ClusterDataImpl, InterceptedService<Channel, ClientInterceptor>>,
    ClusterDataImpl,
)> {
    let auth_channel = grpc_connect(block_engine_url).await?;
    let client_interceptor =
        ClientInterceptor::new(AuthServiceClient::new(auth_channel), auth_keypair).await?;

    let searcher_channel = grpc_connect(block_engine_url).await?;
    let searcher_service_client =
        SearcherServiceClient::with_interceptor(searcher_channel, client_interceptor);

    let cluster_data_impl = ClusterDataImpl::new(
        rpc_pubsub_addr.to_string(),
        searcher_service_client.clone(),
        exit.clone(),
    )
    .await;

    Ok((
        SearcherClient::new(
            cluster_data_impl.clone(),
            searcher_service_client,
            exit.clone(),
        ),
        cluster_data_impl,
    ))
}