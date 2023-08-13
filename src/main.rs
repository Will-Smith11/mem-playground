use ethers::types::H256;
use futures_util::Stream;

use serde::{de::DeserializeOwned, Serialize};
// use ethers::i
use ethers::providers::interval;

use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use std::{fmt::Debug, future::Future, pin::Pin};

use ethers::prelude::PendingTransaction;
use ethers::prelude::ProviderError;
use ethers::prelude::{JsonRpcClient, Middleware, Provider, Transaction, TransactionReceipt};
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::types::BlockId;
use ethers::types::TxHash;
use ethers::types::U64;

use futures_timer::Delay;

#[cfg(target_arch = "wasm32")]
pub(crate) type PinBoxFut<'a, T> = Pin<Box<dyn Future<Output = Result<T, ProviderError>> + 'a>>;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type PinBoxFut<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, ProviderError>> + Send + 'a>>;

// Because this is the exact same struct it will have the exact same memory aliment
// allowing us to bypass the fact that ethers-rs doesn't export this enum normally
// We box the TransactionReceipts to keep the enum small.
#[allow(unused)]
enum PendingTxState<'a> {
    /// Initial delay to ensure the GettingTx loop doesn't immediately fail
    InitialDelay(Pin<Box<Delay>>),

    /// Waiting for interval to elapse before calling API again
    PausedGettingTx,

    /// Polling The blockchain to see if the Tx has confirmed or dropped
    GettingTx(PinBoxFut<'a, Option<Transaction>>),

    /// Waiting for interval to elapse before calling API again
    PausedGettingReceipt,

    /// Polling the blockchain for the receipt
    GettingReceipt(PinBoxFut<'a, Option<TransactionReceipt>>),

    /// If the pending tx required only 1 conf, it will return early. Otherwise it will
    /// proceed to the next state which will poll the block number until there have been
    /// enough confirmations
    CheckingReceipt(Option<TransactionReceipt>),

    /// Waiting for interval to elapse before calling API again
    PausedGettingBlockNumber(Option<TransactionReceipt>),

    /// Polling the blockchain for the current block number
    GettingBlockNumber(PinBoxFut<'a, U64>, Option<TransactionReceipt>),

    /// Future has completed and should panic if polled again
    Completed,
}

#[derive(Debug)]
pub struct TestMiddleware {
    provider: Provider<FakeConn>,
}

#[derive(Debug)]
pub struct FakeConn {}

#[async_trait::async_trait]
impl JsonRpcClient for FakeConn {
    type Error = ProviderError;

    async fn request<T: Serialize + Send + Sync, R: DeserializeOwned>(
        &self,
        method: &str,
        params: T,
    ) -> Result<R, ProviderError> {
        unreachable!()
    }
}

#[async_trait::async_trait]
impl Middleware for TestMiddleware {
    type Provider = FakeConn;
    type Error = ProviderError;
    type Inner = Self;

    fn inner(&self) -> &Self::Inner {
        &self
    }

    fn provider(&self) -> &Provider<Self::Provider> {
        &self.provider
    }

    async fn send_transaction<T: Into<TypedTransaction> + Send + Sync>(
        &self,
        _tx: T,
        _block: Option<BlockId>,
    ) -> Result<PendingTransaction<'_, Self::Provider>, Self::Error> {
        let block = U64::from(1);
        let tx_receipts = TransactionReceipt {
            block_number: Some(block),
            ..Default::default()
        };

        let faked_transaction =
            PendingTransactionMock::new(H256::zero(), self.provider(), block, tx_receipts);

        unsafe { Ok(std::mem::transmute(faked_transaction)) }
    }
}

const DEFAULT_RETRIES: usize = 3;

#[allow(unused)]
pub struct PendingTransactionMock<'a, P> {
    tx_hash: TxHash,
    confirmations: usize,
    provider: &'a Provider<P>,
    state: PendingTxState<'a>,
    interval: Box<dyn Stream<Item = ()> + Send + Unpin>,
    retries_remaining: usize,
}
impl<'a, P: JsonRpcClient> PendingTransactionMock<'a, P> {
    /// Creates a new pending transaction poller from a hash and a provider
    pub fn new(
        tx_hash: TxHash,
        provider: &'a Provider<P>,
        block_number: U64,
        receipts: TransactionReceipt,
    ) -> Self {
        let state = PendingTxState::GettingBlockNumber(
            Box::pin(async move { Ok(block_number) }),
            Some(receipts),
        );

        Self {
            tx_hash,
            confirmations: 0,
            provider,
            state,
            interval: Box::new(interval(provider.get_interval())),
            retries_remaining: DEFAULT_RETRIES,
        }
    }
}

#[tokio::main]
async fn main() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    let prov = Provider::new(FakeConn {});

    let mid = TestMiddleware { provider: prov };

    let fake_tx = TypedTransaction::default();
    let test_fut = mid.send_transaction(fake_tx, None).await.unwrap();
    println!("our modded fut: {test_fut:?}");

    let res = test_fut.await;
    println!("type pwning ftw: {:?}", res);
}
