use gasket::framework::*;
use lapin::{
    options::BasicPublishOptions, BasicProperties, Channel, Connection, ConnectionProperties,
};
use serde::Deserialize;

use crate::framework::*;

pub struct Worker {
    channel: Channel,
}

#[async_trait::async_trait(?Send)]
impl gasket::framework::Worker<Stage> for Worker {
    async fn bootstrap(stage: &Stage) -> Result<Self, WorkerError> {
        let connection = Connection::connect(&stage.config.url, ConnectionProperties::default())
            .await
            .or_panic()?;

        let channel = connection.create_channel().await.or_panic()?;

        Ok(Self { channel })
    }

    async fn schedule(
        &mut self,
        stage: &mut Stage,
    ) -> Result<WorkSchedule<ChainEvent>, WorkerError> {
        let msg = stage.input.recv().await.or_panic()?;
        Ok(WorkSchedule::Unit(msg.payload))
    }

    async fn execute(&mut self, unit: &ChainEvent, stage: &mut Stage) -> Result<(), WorkerError> {
        let point = unit.point().clone();
        let record = unit.record().cloned();

        if record.is_none() {
            return Ok(());
        }

        let payload = serde_json::to_vec(&serde_json::Value::from(record.unwrap())).or_panic()?;

        self.channel
            .basic_publish(
                &stage.config.exchange,
                &stage.config.routing_key.clone().unwrap_or_default(),
                BasicPublishOptions::default(),
                &payload,
                BasicProperties::default(),
            )
            .await
            .or_retry()?;

        stage.ops_count.inc(1);
        stage.latest_block.set(point.slot_or_default() as i64);
        stage.cursor.add_breadcrumb(point.clone());

        Ok(())
    }
}

#[derive(Stage)]
#[stage(name = "sink-rabbitmq", unit = "ChainEvent", worker = "Worker")]
pub struct Stage {
    config: Config,
    cursor: Cursor,

    pub input: MapperInputPort,

    #[metric]
    ops_count: gasket::metrics::Counter,

    #[metric]
    latest_block: gasket::metrics::Gauge,
}

#[derive(Default, Debug, Deserialize)]
pub struct Config {
    pub url: String,
    pub exchange: String,
    pub routing_key: Option<String>,
}

impl Config {
    pub fn bootstrapper(self, ctx: &Context) -> Result<Stage, Error> {
        let stage = Stage {
            config: self,
            cursor: ctx.cursor.clone(),
            ops_count: Default::default(),
            latest_block: Default::default(),
            input: Default::default(),
        };

        Ok(stage)
    }
}
