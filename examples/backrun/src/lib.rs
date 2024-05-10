use std::{
    ffi::CStr,
    panic::{self, PanicInfo},
    process, slice,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use env_logger::TimestampPrecision;
use jito_protos::{
    auth::auth_service_client::AuthServiceClient, bundle::rejected::Reason,
    searcher::searcher_service_client::SearcherServiceClient,
};
use jito_searcher_client::{
    client_interceptor::ClientInterceptor, cluster_data_impl::ClusterDataImpl, grpc_connect,
    SearcherClient, SearcherClientResult,
};

use solana_sdk::signature::{read_keypair_file, Keypair};

use async_ffi::async_ffi;
use async_ffi::{FfiFuture, FutureExt};
use jito_protos::bundle::bundle_result::Result;
use jito_protos::bundle::rejected::Reason::SimulationFailure;
use tonic::{codegen::InterceptedService, transport::Channel};

// use async_ffi::{FfiFuture, FutureExt};
// use tonic::{transport::Channel, Client};

#[no_mangle]
pub extern "C" fn send_bundle_with_confirmation(
    block_engine_url: *const i8,
    auth_keypair_path: *const i8,
    rpc_ws_url: *const i8,
    wire_transaction: *const u8,
    wire_transaction_len: usize,
) -> bool {
    let block_engine_url = unsafe { CStr::from_ptr(block_engine_url).to_str().unwrap() };
    let auth_keypair_path = unsafe { CStr::from_ptr(auth_keypair_path).to_str().unwrap() };
    let rpc_ws_url = unsafe { CStr::from_ptr(rpc_ws_url).to_str().unwrap() };
    let wire_transaction = unsafe { slice::from_raw_parts(wire_transaction, wire_transaction_len) };

    let auth_keypair =
        Arc::new(read_keypair_file(auth_keypair_path).expect("reads keypair at path"));
    let vec_of_vecs: Vec<Vec<u8>> = vec![wire_transaction.to_vec()];

    let exit = graceful_panic(None);

    // print!("block_engine_url: {:?}", block_engine_url);
    // print!("auth_keypair_path: {:?}", auth_keypair_path);
    // print!("rpc_ws_url: {:?}", rpc_ws_url);
    // print!("wire_transaction: {:?}", wire_transaction);

    // Now you can call your async function
    let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut tx_accepted = false;
        let searcher_client_service =
            get_searcher_client(&auth_keypair, &exit, block_engine_url, rpc_ws_url)
                .await
                .unwrap();
        let (mut searcher_client, cluster_data) = searcher_client_service;

        let txs: &[Vec<u8>] = &vec_of_vecs;
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
    });

    result
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
