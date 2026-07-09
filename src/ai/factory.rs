use anyhow::Result;

use crate::ai::adapter::AIAdapter;
use crate::ai::mlx_adapter::MlxAdapter;

pub fn create_runtime_adapter() -> Result<Box<dyn AIAdapter>> {
    let adapter = MlxAdapter::new();
    adapter.health_check()?;
    Ok(Box::new(adapter))
}
