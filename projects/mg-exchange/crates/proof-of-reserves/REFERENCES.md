# Proof of Reserves — Business Logic References

Traces Merkle tree proof of reserves to industry standards.

## 17. Proof of Reserves -- Merkle Tree

### Mechanism

Cryptographic proof that the exchange holds sufficient assets to cover all user liabilities.

### A. Mathematical Derivation

**Leaf hash:**
```
leaf = SHA256(user_id || ":" || asset || ":" || balance)
```

**Internal node hash (canonical ordering):**
```
node = SHA256(min(left, right) || max(left, right))
```

Canonical ordering makes the tree deterministic regardless of insertion order.

**Proof verification:** Given a leaf hash `h`, proof path `[(sibling_hash, position)]`:

```
current = h
for (sibling, position) in proof:
    if position == Left:
        current = SHA256(min(sibling, current) || max(sibling, current))
    else:
        current = SHA256(min(current, sibling) || max(current, sibling))
assert(current == root)
```

### B. Worked Numerical Example

Three users:
```
leaf_A = SHA256("alice:BTC:1.5") = 0xab12...
leaf_B = SHA256("bob:BTC:0.3")   = 0xcd34...
leaf_C = SHA256("carol:BTC:2.0") = 0xef56...
```

Tree (padding leaf_C with itself since 3 is odd):
```
           root = SHA256(min(h_AB, h_CC) || max(h_AB, h_CC))
          /                                           \
h_AB = SHA256(min(leaf_A, leaf_B) || max(...))     h_CC = SHA256(min(leaf_C, leaf_C) || ...)
    /       \                                        |
 leaf_A   leaf_B                                   leaf_C
```

Alice's inclusion proof: `[(leaf_B, Right), (h_CC, Right)]`.
Verification: `hash(leaf_A, leaf_B) -> h_AB`, then `hash(h_AB, h_CC) -> root`. Match.

### C. Reference Implementation Comparison

Identical to the Maxwell/Dagher approach used by Kraken and OKX. Our canonical ordering matches Chainlink's implementation.

### D. Edge Cases & Boundary Behavior

- **Single user:** Tree is just the leaf. Proof is empty.
- **Odd number of leaves:** Last leaf is paired with itself.
- **Zero balance:** Valid leaf; included in the tree.

### E. Test Coverage Reference

- **File:** `crates/proof-of-reserves/src/merkle.rs` (inline tests)

### Compliance

**Match.** Standard Merkle tree proof of reserves.
