#!/bin/bash

# Set a timeout in seconds for each test (adjust as needed)
TIMEOUT=30

# List all test names (excluding ignored and benchmarks)
TESTS=$(cargo test -- --list | grep 'test$' | awk '{print $1}')

# Arrays to store results
declare -a PASSED
declare -a FAILED

echo "Discovered tests:"
echo "$TESTS"
echo

for TEST in $TESTS; do
    echo "Running test: $TEST"
    if timeout $TIMEOUT cargo test "$TEST" -- --nocapture --test-threads=1; then
        echo "✅ $TEST passed"
        PASSED+=("$TEST")
    else
        echo "❌ $TEST failed or timed out"
        FAILED+=("$TEST")
    fi
    echo "----------------------------------------"
done

echo

echo "========== Test Summary =========="
echo "Passed tests:"
for t in "${PASSED[@]}"; do
    echo "  ✅ $t"
done

echo

echo "Failed or Timed Out tests:"
for t in "${FAILED[@]}"; do
    echo "  ❌ $t"
done

echo "==================================="
