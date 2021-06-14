use crate::{
    cmd::*,
    keypair::Keypair,
    result::Result,
    traits::{TxnEnvelope, TxnFee, TxnSign},
};
use serde::Deserialize;

#[derive(Debug, StructOpt)]
/// Onboard one (or more) validators  with this wallet.
///
/// The payment is not submitted to the system unless the '--commit' option is
/// given.
///
/// Note that multiple staking transactions are submitted individually and not as a
/// single transaction. Any failures will abort the remaining staking entries.
pub struct Cmd {
    #[structopt(flatten)]
    stake: Type,

    /// Manually set fee to pay for the transaction(s)
    #[structopt(long)]
    fee: Option<u64>,

    /// Whether to commit the transaction(s) to the blockchain
    #[structopt(long)]
    commit: bool,
}

#[derive(Debug, StructOpt)]
enum Type {
    /// Stake a single validator
    One(Validator),
    /// Stake multiple validators via file import.
    /// Check "helium-wallet validator stake multi --help" for details
    Multi(File),
}

#[derive(Debug, StructOpt)]
/// The input file for multiple validator stakes is expected to be json file
/// with a list of address and staking amounts. For example:
///
/// [
///     {
///         "address": "<adddress1>",
///         "stake": 10000,
///     },
///     {
///         "address": "<adddress2>",
///         "stake": 10000
///     }
/// ]
struct File {
    path: PathBuf,
}

impl Cmd {
    pub async fn run(&self, opts: Opts) -> Result {
        let validators = self.collect_validators()?;

        let password = get_password(false)?;
        let wallet = load_wallet(opts.files)?;
        let keypair = wallet.decrypt(password.as_bytes())?;

        let client = helium_api::Client::new_with_base_url(api_url(wallet.public_key.network));
        let fee_config = if self.fee.is_none() {
            Some(get_txn_fees(&client).await?)
        } else {
            None
        };

        for validator in validators {
            let txn = self.mk_txn(&keypair, &fee_config, &validator)?;
            let envelope = txn.in_envelope();
            let status = maybe_submit_txn(self.commit, &client, &envelope).await?;
            print_txn(&envelope, &txn, &status, &opts.format)?
        }
        Ok(())
    }

    fn mk_txn(
        &self,
        keypair: &Keypair,
        fee_config: &Option<TxnFeeConfig>,
        validator: &Validator,
    ) -> Result<BlockchainTxnStakeValidatorV1> {
        let mut txn = BlockchainTxnStakeValidatorV1 {
            address: validator.address.to_vec(),
            owner: keypair.public_key().to_vec(),
            stake: u64::from(validator.stake),
            fee: 0,
            owner_signature: vec![],
        };
        txn.fee = if let Some(fee) = self.fee {
            fee
        } else {
            txn.txn_fee(fee_config.as_ref().unwrap())?
        };
        txn.owner_signature = txn.sign(keypair)?;
        Ok(txn)
    }

    fn collect_validators(&self) -> Result<Vec<Validator>> {
        match &self.stake {
            Type::One(validator) => Ok(vec![validator.clone()]),
            Type::Multi(file) => {
                let file = std::fs::File::open(file.path.clone())?;
                let validators: Vec<Validator> = serde_json::from_reader(file)?;
                Ok(validators)
            }
        }
    }
}

fn print_txn(
    envelope: &BlockchainTxn,
    txn: &BlockchainTxnStakeValidatorV1,
    status: &Option<PendingTxnStatus>,
    format: &OutputFormat,
) -> Result {
    let validator = PublicKey::from_bytes(&txn.address)?.to_string();
    match format {
        OutputFormat::Table => {
            ptable!(
                ["Key", "Value"],
                ["Validator", validator],
                ["Stake (HNT)", Hnt::from(txn.stake)],
                ["Fee (DC)", txn.fee],
                ["Hash", status_str(status)],
                [Frb => "WARNING",
                "Once staked an owner cannot access the staked amount until\n\
                250,000 blocks (approx. 5 months) after unstaking."]
            );
            print_footer(status)
        }
        OutputFormat::Json => {
            let table = json!({
                "validator" : validator,
                "fee": txn.fee,
                "staking_fee": txn.stake,
                "txn": envelope.to_b64()?,
                "hash": status_json(status)
            });
            print_json(&table)
        }
    }
}

#[derive(Debug, Deserialize, StructOpt, Clone)]
pub struct Validator {
    address: PublicKey,
    stake: Hnt,
}
