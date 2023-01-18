use crate::{Entropy, Error, Result};
use helium_proto::{services::poc_iot, BlockchainRegionParamV1, DataRate};
use rand::{seq::SliceRandom, Rng, SeedableRng};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_BEACON_V0_PAYLOAD_SIZE: usize = 10;
pub const MIN_BEACON_V0_PAYLOAD_SIZE: usize = 5;

// Supported datarates worldwide. Note that SF12 is not supported everywhere 
pub const BEACON_DATA_RATES: &[DataRate] = &[
    DataRate::Sf7bw125,
    DataRate::Sf8bw125,
    DataRate::Sf9bw125,
    DataRate::Sf10bw125,
];

#[derive(Debug, Clone)]
pub struct Beacon {
    pub data: Vec<u8>,

    pub frequency: u64,
    pub datarate: DataRate,
    pub remote_entropy: Entropy,
    pub local_entropy: Entropy,
}

impl Beacon {
    /// Construct a new beacon with a given remote and local entropy. The remote
    /// and local entropy are checked for version equality.
    ///
    /// Version 0 beacons use a Sha256 of the remote and local entropy (data and
    /// timestamp), which is then used as a 32 byte seed to a ChaCha12 rng. This
    /// rng is used to choose a random frequency from the given region
    /// parameters and a payload size between MIN_BEACON_V0_PAYLOAD_SIZE and
    /// MAX_BEACON_V0_PAYLOAD_SIZE.
    pub fn new(
        remote_entropy: Entropy,
        local_entropy: Entropy,
        region_params: &[BlockchainRegionParamV1],
    ) -> Result<Self> {
        match remote_entropy.version {
            0 | 1 => {
                let mut data = {
                    let mut hasher = Sha256::new();
                    remote_entropy.digest(&mut hasher);
                    local_entropy.digest(&mut hasher);
                    hasher.finalize().to_vec()
                };

                // Construct a 32 byte seed from the hash of the local and
                // remote entropy
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&data[0..32]);
                // Make a random generator
                let mut rng = rand_chacha::ChaCha12Rng::from_seed(seed);

                // And pick freqyency, payload_size and data_rate. Note that the
                // ordering matters since the random number generator is used in
                // this order.
                let frequency = rand_frequency(region_params, &mut rng)?;
                let payload_size =
                    rng.gen_range(MIN_BEACON_V0_PAYLOAD_SIZE..=MAX_BEACON_V0_PAYLOAD_SIZE);

                let datarate = rand_data_rate(BEACON_DATA_RATES, &mut rng)?;

                Ok(Self {
                    data: {
                        data.truncate(payload_size);
                        data
                    },
                    frequency,
                    datarate: datarate.to_owned(),
                    local_entropy,
                    remote_entropy,
                })
            }
            _ => Err(Error::invalid_version()),
        }
    }

    pub fn beacon_id(&self) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(&self.data)
    }
}

fn rand_frequency<R>(region_params: &[BlockchainRegionParamV1], rng: &mut R) -> Result<u64>
where
    R: Rng + ?Sized,
{
    region_params
        .choose(rng)
        .map(|params| params.channel_frequency)
        .ok_or_else(Error::no_region_params)
}

fn rand_data_rate<'a, R>(data_rates: &'a [DataRate], rng: &mut R) -> Result<&'a DataRate>
where
    R: Rng + ?Sized,
{
    data_rates.choose(rng).ok_or_else(Error::no_data_rate)
}

impl TryFrom<Beacon> for poc_iot::IotBeaconReportReqV1 {
    type Error = Error;
    fn try_from(v: Beacon) -> Result<Self> {
        Ok(Self {
            pub_key: vec![],
            local_entropy: v.local_entropy.data,
            remote_entropy: v.remote_entropy.data,
            data: v.data,
            frequency: v.frequency,
            channel: 0,
            datarate: v.datarate as i32,
            tmst: 0,
            tx_power: 27,
            // The timestamp of the beacon is the timestamp of creation of the
            // report (in nanos)
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(Error::from)?
                .as_nanos() as u64,
            signature: vec![],
        })
    }
}
