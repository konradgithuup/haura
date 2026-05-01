#!/bin/bash

export ROOT="/home/pzittlau/University/ScTP/haura/betree/haura-benchmarks"
export BETREE_CONFIG="$ROOT/perf-config.json"
CONFIG="perf-config.json"
RESULTS="tuning_results.csv"

# Ranges
COOLING=(0.8 0.9 0.95 0.99)
SPREAD=(0.01 0.05 0.1 0.2 0.3 0.5 1.0)
MIN_CHANGE=(0.01)
MAX_IDLE=(0.5)

echo "cooling,spread,min_change,max_idle,ops_per_sec" > $RESULTS

for c in "${COOLING[@]}"; do
    for s in "${SPREAD[@]}"; do
        for mc in "${MIN_CHANGE[@]}"; do
            for mi in "${MAX_IDLE[@]}"; do
                echo "Testing: C=$c S=$s MC=$mc MI=$mi"

                # Patch JSON
                jq ".optimizer = {
                    cooling_factor: $c,
                    mutation_spread: $s,
                    min_change_ratio: $mc,
                    max_idle_ratio: $mi
                } | .cache_policy = \"WHATT\"" $CONFIG > tmp.json && mv tmp.json $CONFIG

                # Wipe state
                rm -f haura.db proc.jsonl sysinfo.jsonl ycsb_c.csv betree-metrics.jsonl
                truncate --size 4G ./haura.db
                ./target/release/betree-perf ycsb-c 6442450944 0 8 30 > /dev/null 2>&1

                # Parse local result
                if [ -f "ycsb_c.csv" ]; then
                    OPS=$(tail -n 1 "ycsb_c.csv" | awk -F',' '{print $2/($3/1000000000)}')
                    echo "$c,$s,$mc,$mi,$OPS" >> $RESULTS
                    echo "Result: $OPS ops/s"
                else
                    echo "Error: ycsb_c.csv not produced"
                fi
                rm -f haura.db proc.jsonl sysinfo.jsonl ycsb_c.csv betree-metrics.jsonl
            done
        done
    done
done
