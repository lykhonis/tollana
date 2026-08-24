use crate::error::HostError;
use crate::plugin::{Plugin, PluginResult};
use crate::schema::{function_type, parse_package_schema, RANDOM_SCHEMA_BYTES};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};
use tollana_core::{CapHandle, FunctionType, Label, Value};

pub const METHOD_NEXT: u32 = 0;

const MODE_SEEDED: u8 = 0;
const MODE_SECURE: u8 = 1;
const MODE_REPLAY: u8 = 2;

#[derive(Clone, Debug)]
enum Source {
    Seeded {
        state: u64,
    },
    Secure {
        mix: u64,
    },
    Replay {
        samples: Vec<(u32, i64)>,
        index: usize,
    },
}

#[derive(Clone, Debug)]
pub struct Random {
    source: Source,
    samples: Vec<(u32, i64)>,
}

impl Random {
    pub fn seeded(seed: u64) -> Self {
        Self {
            source: Source::Seeded { state: seed },
            samples: Vec::new(),
        }
    }

    pub fn secure() -> Self {
        Self {
            source: Source::Secure { mix: 0 },
            samples: Vec::new(),
        }
    }

    pub fn replay(samples: Vec<(u32, i64)>) -> Self {
        Self {
            source: Source::Replay { samples, index: 0 },
            samples: Vec::new(),
        }
    }

    fn next_bits(&mut self) -> Result<i64, HostError> {
        match &mut self.source {
            Source::Seeded { state } => Ok(splitmix64(state) as i64),
            Source::Secure { mix } => {
                *mix = mix.wrapping_add(1);
                Ok(secure_bits(*mix) as i64)
            }
            Source::Replay { samples, index } => {
                let (method_id, value) = samples
                    .get(*index)
                    .copied()
                    .ok_or_else(|| HostError::new("random_replay_exhausted"))?;
                if method_id != METHOD_NEXT {
                    return Err(HostError::new("random_replay_method_mismatch"));
                }
                *index += 1;
                Ok(value)
            }
        }
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn secure_bits(mix: u64) -> u64 {
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(mix);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    hasher.write_u64(now);
    hasher.finish()
}

impl Plugin for Random {
    fn name(&self) -> &str {
        "random"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> &[u8] {
        RANDOM_SCHEMA_BYTES
    }

    fn methods(&self) -> Result<Vec<(u32, FunctionType)>, HostError> {
        let schema = parse_package_schema(RANDOM_SCHEMA_BYTES)?;
        schema
            .methods
            .iter()
            .map(|m| Ok((m.id, function_type(m)?)))
            .collect()
    }

    fn invoke(
        &mut self,
        method_id: u32,
        args: &[Value],
        _caps: &[CapHandle],
        _ctx: &mut dyn crate::plugin::PluginContext,
    ) -> Result<PluginResult, HostError> {
        if method_id != METHOD_NEXT {
            return Err(HostError::new(format!("unknown random method {method_id}")));
        }
        if !args.is_empty() {
            return Err(HostError::new("random.next takes no arguments"));
        }
        let value = self.next_bits()?;
        self.samples.push((method_id, value));
        Ok(PluginResult::Immediate(vec![Value::i64(
            value,
            Label::Public,
        )]))
    }

    fn snapshot_state(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(1);
        match &self.source {
            Source::Seeded { state } => {
                buf.push(MODE_SEEDED);
                buf.extend_from_slice(&state.to_le_bytes());
                buf.extend_from_slice(&0u32.to_le_bytes());
            }
            Source::Secure { mix } => {
                buf.push(MODE_SECURE);
                buf.extend_from_slice(&mix.to_le_bytes());
                buf.extend_from_slice(&0u32.to_le_bytes());
            }
            Source::Replay { samples, index } => {
                buf.push(MODE_REPLAY);
                buf.extend_from_slice(&0u64.to_le_bytes());
                let start = (*index).min(samples.len());
                let rest = &samples[start..];
                buf.extend_from_slice(&(rest.len() as u32).to_le_bytes());
                for (method_id, value) in rest {
                    buf.extend_from_slice(&method_id.to_le_bytes());
                    buf.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        buf
    }

    fn restore_state(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        if bytes.len() < 13 || bytes[0] != 1 {
            return Err(HostError::new("invalid random snapshot blob"));
        }
        let mode = bytes[1];
        let mut word = [0u8; 8];
        word.copy_from_slice(&bytes[2..10]);
        let word = u64::from_le_bytes(word);
        let rest_len = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
        let mut pos = 14;
        let mut rest = Vec::with_capacity(rest_len);
        for _ in 0..rest_len {
            if pos + 12 > bytes.len() {
                return Err(HostError::new("truncated random snapshot blob"));
            }
            let method_id = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
            pos += 4;
            let value = i64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap());
            pos += 8;
            rest.push((method_id, value));
        }
        if pos != bytes.len() {
            return Err(HostError::new("trailing random snapshot bytes"));
        }
        self.source = match mode {
            MODE_SEEDED => Source::Seeded { state: word },
            MODE_SECURE => Source::Secure { mix: word },
            MODE_REPLAY => Source::Replay {
                samples: rest,
                index: 0,
            },
            _ => return Err(HostError::new("invalid random snapshot mode")),
        };
        Ok(())
    }

    fn recorded_samples(&self) -> Vec<(u32, i64)> {
        self.samples.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{Clock, METHOD_NOW_WALL};
    use crate::host::Host;
    use tollana_core::{ExecOutcome, Label};

    fn next_module(plugin_id: u32) -> String {
        format!(
            r#"
(module
  (host.import random.next
    (pluginId {plugin_id})
    (methodId 0)
    (result i64))
  (func (export "main") (result i64)
    (host.invoke random.next)))
"#
        )
    }

    fn clock_random_module(clock_id: u32, random_id: u32) -> String {
        format!(
            r#"
(module
  (host.import clock.now_wall
    (pluginId {clock_id})
    (methodId 0)
    (result i64))
  (host.import random.next
    (pluginId {random_id})
    (methodId 0)
    (result i64))
  (func (export "main") (result i64)
    (i64.add
      (host.invoke clock.now_wall)
      (host.invoke random.next))))
"#
        )
    }

    #[test]
    fn seeded_is_deterministic() {
        let mut a = Host::new();
        a.register(Box::new(Random::seeded(7))).unwrap();
        a.bind().unwrap();
        let id = a.plugin_id("random").unwrap();
        a.instantiate_text(&next_module(id)).unwrap();
        let first = match a.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => results,
            other => panic!("{other:?}"),
        };

        let mut b = Host::new();
        b.register(Box::new(Random::seeded(7))).unwrap();
        b.bind().unwrap();
        b.instantiate_text(&next_module(id)).unwrap();
        match b.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => assert_eq!(results, first),
            other => panic!("{other:?}"),
        }
        assert_eq!(a.plugin_samples("random"), b.plugin_samples("random"));
    }

    #[test]
    fn snapshot_restores_prng_state() {
        let mut host = Host::new();
        host.register(Box::new(Random::seeded(99))).unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("random").unwrap();
        host.instantiate_text(&next_module(id)).unwrap();
        let first = match host.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => results[0],
            other => panic!("{other:?}"),
        };
        let bytes = host.snapshot().unwrap();

        let mut host2 = Host::new();
        host2.register(Box::new(Random::seeded(0))).unwrap();
        host2.restore(&bytes).unwrap();
        let second = match host2.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => results[0],
            other => panic!("{other:?}"),
        };

        let mut sequential = Random::seeded(99);
        let _ = sequential.next_bits().unwrap();
        let expected = sequential.next_bits().unwrap();
        assert_ne!(first, Value::i64(expected, Label::Public));
        assert_eq!(second, Value::i64(expected, Label::Public));
    }

    #[test]
    fn clock_and_random_strict_replay_from_journal() {
        let mut live = Host::new();
        live.register(Box::new(Clock::wall())).unwrap();
        live.register(Box::new(Random::secure())).unwrap();
        live.bind().unwrap();
        let clock_id = live.plugin_id("clock").unwrap();
        let random_id = live.plugin_id("random").unwrap();
        let src = clock_random_module(clock_id, random_id);
        live.instantiate_text(&src).unwrap();
        let first = match live.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => results,
            other => panic!("{other:?}"),
        };
        let clock_samples = live.journal_plugin_results("clock");
        let random_samples = live.journal_plugin_results("random");
        assert_eq!(clock_samples.len(), 1);
        assert_eq!(clock_samples[0].0, METHOD_NOW_WALL);
        assert_eq!(random_samples, live.plugin_samples("random"));

        let mut replay = Host::new();
        replay
            .register(Box::new(Clock::replay(clock_samples)))
            .unwrap();
        replay
            .register(Box::new(Random::replay(random_samples)))
            .unwrap();
        replay.bind().unwrap();
        assert_eq!(replay.plugin_id("clock"), Some(clock_id));
        assert_eq!(replay.plugin_id("random"), Some(random_id));
        replay.instantiate_text(&src).unwrap();
        match replay.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => assert_eq!(results, first),
            other => panic!("{other:?}"),
        }
    }
}
