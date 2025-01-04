use crate::{BloomError, BloomFilterStorage, Result};
use redis::{Client, Commands, Connection};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

pub struct RedisStorage {
    conn: Mutex<Connection>,
    capacity: usize,
    max_levels: usize,
    prefix: String,
}

impl RedisStorage {
    pub fn new(
        redis_url: &str,
        capacity: usize,
        max_levels: usize,
        prefix: &str,
    ) -> Result<Self> {
        let client = Client::open(redis_url).map_err(|e| {
            BloomError::StorageError(format!("Redis connection error: {}", e))
        })?;

        let mut conn = client.get_connection().map_err(|e| {
            BloomError::StorageError(format!("Redis connection error: {}", e))
        })?;

        // Initialize timestamps for each level
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|e| BloomError::StorageError(e.to_string()))?
            .as_secs();

        for level in 0..max_levels {
            let ts_key = format!("{}:ts:{}", prefix, level);
            let _: () = redis::cmd("SETNX")
                .arg(&ts_key)
                .arg(now.to_string())
                .query(&mut conn)
                .map_err(|e| {
                    BloomError::StorageError(format!("Redis error: {}", e))
                })?;
        }

        Ok(Self {
            conn: Mutex::new(conn),
            capacity,
            max_levels,
            prefix: prefix.to_string(),
        })
    }

    fn bits_key(&self, level: usize) -> String {
        format!("{}:bits:{}", self.prefix, level)
    }

    fn ts_key(&self, level: usize) -> String {
        format!("{}:ts:{}", self.prefix, level)
    }
}

impl BloomFilterStorage for RedisStorage {
    fn set_bits(&mut self, level: usize, indices: &[usize]) -> Result<()> {
        if level >= self.max_levels {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        }

        if let Some(&max_index) = indices.iter().max() {
            if max_index >= self.capacity {
                return Err(BloomError::IndexOutOfBounds {
                    index: max_index,
                    capacity: self.capacity,
                });
            }
        }

        let key = self.bits_key(level);

        let mut pipe = redis::pipe();
        pipe.atomic();

        let mut conn = self.conn.lock().map_err(|e| {
            BloomError::StorageError(format!("Redis lock error: {}", e))
        })?;

        for &index in indices {
            pipe.cmd("SETBIT").arg(&key).arg(index).arg(1);
        }

        let _: () = pipe.query(&mut conn).map_err(|e| {
            BloomError::StorageError(format!("Redis error: {}", e))
        })?;

        Ok(())
    }

    fn get_bits(&self, level: usize, indices: &[usize]) -> Result<Vec<bool>> {
        if level >= self.max_levels {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        }

        if let Some(&max_index) = indices.iter().max() {
            if max_index >= self.capacity {
                return Err(BloomError::IndexOutOfBounds {
                    index: max_index,
                    capacity: self.capacity,
                });
            }
        }

        let key = self.bits_key(level);
        let mut conn = self.conn.lock().map_err(|e| {
            BloomError::StorageError(format!("Redis lock error: {}", e))
        })?;

        // For get_bits, we need to execute commands one by one since we can't use mut self
        let mut results = Vec::with_capacity(indices.len());
        for &index in indices {
            let value: i32 = redis::cmd("GETBIT")
                .arg(&key)
                .arg(index)
                .query(&mut conn)
                .map_err(|e| {
                    BloomError::StorageError(format!("Redis error: {}", e))
                })?;
            results.push(value == 1);
        }

        Ok(results)
    }

    fn clear_level(&mut self, level: usize) -> Result<()> {
        if level >= self.max_levels {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        }
        let mut conn = self.conn.lock().map_err(|e| {
            BloomError::StorageError(format!("Redis lock error: {}", e))
        })?;

        let key = self.bits_key(level);
        let _: () = conn.del(&key).map_err(|e| {
            BloomError::StorageError(format!("Redis error: {}", e))
        })?;

        Ok(())
    }

    fn set_timestamp(
        &mut self,
        level: usize,
        timestamp: SystemTime,
    ) -> Result<()> {
        if level >= self.max_levels {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        }
        let mut conn = self.conn.lock().map_err(|e| {
            BloomError::StorageError(format!("Redis lock error: {}", e))
        })?;

        let key = self.ts_key(level);
        let secs = timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|e| BloomError::StorageError(e.to_string()))?
            .as_secs();

        let _: () = conn.set(&key, secs.to_string()).map_err(|e| {
            BloomError::StorageError(format!("Redis error: {}", e))
        })?;

        Ok(())
    }

    fn get_timestamp(&self, level: usize) -> Result<Option<SystemTime>> {
        if level >= self.max_levels {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        }

        let mut conn = self.conn.lock().map_err(|e| {
            BloomError::StorageError(format!("Redis lock error: {}", e))
        })?;

        let key = self.ts_key(level);
        let secs_str: Option<String> =
            redis::cmd("GET").arg(&key).query(&mut conn).map_err(|e| {
                BloomError::StorageError(format!("Redis error: {}", e))
            })?;

        match secs_str {
            Some(s) => {
                let secs = s.parse::<u64>().map_err(|e| {
                    BloomError::StorageError(format!(
                        "Invalid timestamp format: {}",
                        e
                    ))
                })?;
                Ok(Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)))
            }
            None => Ok(None),
        }
    }

    fn num_levels(&self) -> usize {
        self.max_levels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{default_hash_function, SlidingBloomFilter};
    use dotenvy::dotenv;
    use rand::RngCore;
    use std::env;
    use std::time::Instant;

    fn get_redis_url() -> String {
        dotenv().ok();
        env::var("REDIS_URI").unwrap_or_else(|_| "redis://127.0.0.1/".to_string())
    }

    #[test]
    fn test_redis_batch_performance() {
        const NUM_ITEMS: usize = 1_000;

        // Setup Redis storage
        let mut bloom = SlidingBloomFilter::new(
            RedisStorage::new(&get_redis_url(), NUM_ITEMS, 3, "test_filter")
                .unwrap(),
            NUM_ITEMS,
            0.01,
            Duration::from_secs(60),
            3,
            default_hash_function,
        )
        .unwrap();

        // Generate test data
        let mut rng = rand::thread_rng();
        let items: Vec<Vec<u8>> = (0..NUM_ITEMS)
            .map(|_| {
                let mut bytes = vec![0u8; 16];
                rng.fill_bytes(&mut bytes);
                bytes
            })
            .collect();

        // Measure insertion performance
        println!("Starting batch insertion test...");
        let start = Instant::now();

        for (i, item) in items.iter().enumerate() {
            bloom.insert(item).unwrap();

            if (i + 1) % 100 == 0 {
                println!("Inserted {} items...", i + 1);
            }
        }

        let insert_elapsed = start.elapsed();
        println!(
            "\nBatch insertion of {} items took: {:?}",
            NUM_ITEMS, insert_elapsed
        );
        println!(
            "Average insertion time per item: {:?}",
            insert_elapsed / NUM_ITEMS as u32
        );

        // Measure query performance
        println!("\nStarting batch query test...");
        let start = Instant::now();

        for (i, item) in items.iter().enumerate() {
            assert!(bloom.query(item).unwrap());

            if (i + 1) % 100 == 0 {
                println!("Queried {} items...", i + 1);
            }
        }

        let query_elapsed = start.elapsed();
        println!(
            "\nBatch query of {} items took: {:?}",
            NUM_ITEMS, query_elapsed
        );
        println!(
            "Average query time per item: {:?}",
            query_elapsed / NUM_ITEMS as u32
        );

        // Performance assertions
        assert!(
            insert_elapsed < Duration::from_secs(5),
            "Insertion of {} items took {:?}, which is more than 1 second",
            NUM_ITEMS,
            insert_elapsed
        );

        assert!(
            query_elapsed < Duration::from_secs(5),
            "Querying {} items took {:?}, which is more than 1 second",
            NUM_ITEMS,
            query_elapsed
        );
    }
}
