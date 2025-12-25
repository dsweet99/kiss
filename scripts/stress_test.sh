#!/bin/bash
# Stress test for kiss - generates many files and measures performance

set -e

# Configuration
NUM_FILES=${1:-100}
TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT

echo "=== Kiss Stress Test ==="
echo "Generating $NUM_FILES Python files in $TEMP_DIR..."

# Generate Python files with varying complexity
for i in $(seq 1 $NUM_FILES); do
    cat > "$TEMP_DIR/module_$i.py" << 'PYEOF'
import os
import sys
from typing import List, Optional

class DataProcessor:
    def __init__(self, config: dict):
        self.config = config
        self.data = []
        self.cache = {}
    
    def process(self, items: List[str]) -> List[str]:
        results = []
        for item in items:
            if item in self.cache:
                results.append(self.cache[item])
            else:
                processed = self._transform(item)
                self.cache[item] = processed
                results.append(processed)
        return results
    
    def _transform(self, item: str) -> str:
        return item.upper()

def calculate_metrics(data: List[int]) -> dict:
    if not data:
        return {"count": 0, "sum": 0, "avg": 0}
    total = sum(data)
    count = len(data)
    return {
        "count": count,
        "sum": total,
        "avg": total / count,
    }

def validate_input(value: str) -> bool:
    if not value:
        return False
    if len(value) > 100:
        return False
    return True
PYEOF
done

# Also generate some Rust files
echo "Generating $((NUM_FILES / 2)) Rust files..."
for i in $(seq 1 $((NUM_FILES / 2))); do
    cat > "$TEMP_DIR/module_$i.rs" << 'RSEOF'
use std::collections::HashMap;

pub struct Processor {
    config: HashMap<String, String>,
    cache: HashMap<String, String>,
}

impl Processor {
    pub fn new() -> Self {
        Self {
            config: HashMap::new(),
            cache: HashMap::new(),
        }
    }

    pub fn process(&mut self, items: Vec<String>) -> Vec<String> {
        items.into_iter()
            .map(|item| {
                if let Some(cached) = self.cache.get(&item) {
                    cached.clone()
                } else {
                    let result = item.to_uppercase();
                    self.cache.insert(item, result.clone());
                    result
                }
            })
            .collect()
    }
}

pub fn calculate_metrics(data: &[i32]) -> (usize, i32, f64) {
    if data.is_empty() {
        return (0, 0, 0.0);
    }
    let sum: i32 = data.iter().sum();
    let count = data.len();
    (count, sum, sum as f64 / count as f64)
}
RSEOF
done

echo ""
echo "Generated files:"
echo "  Python: $NUM_FILES files"
echo "  Rust:   $((NUM_FILES / 2)) files"
echo "  Total:  $((NUM_FILES + NUM_FILES / 2)) files"
echo ""

# Build release binary if needed
if [ ! -f ./target/release/kiss ]; then
    echo "Building release binary..."
    cargo build --release 2>/dev/null
fi

# Warm-up run
echo "Warm-up run..."
./target/release/kiss "$TEMP_DIR" --all > /dev/null 2>&1 || true

# Timed runs
echo ""
echo "=== Benchmark (3 runs) ==="
for run in 1 2 3; do
    start=$(python3 -c 'import time; print(time.time())')
    ./target/release/kiss "$TEMP_DIR" --all > /dev/null 2>&1 || true
    end=$(python3 -c 'import time; print(time.time())')
    duration=$(python3 -c "print(f'{($end - $start):.3f}')")
    echo "Run $run: ${duration}s"
done

echo ""
echo "=== Profile with 'time' ==="
time ./target/release/kiss "$TEMP_DIR" --all > /dev/null 2>&1 || true

echo ""
echo "To test with more files: $0 500"

