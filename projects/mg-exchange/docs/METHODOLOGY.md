# Development Methodology — Research First, Code Second

## Principle

NEVER implement before understanding how industry leaders do it.
Every major system follows this cycle:

```
RESEARCH → DESIGN → IMPLEMENT → VERIFY → ITERATE
```

NOT:
```
IMPLEMENT → HOPE IT WORKS → PATCH BUGS
```

## Applied to Each Sprint

### Before Writing Code:
1. Study 3+ real exchange implementations (Binance, Bybit, OKX, open source)
2. Document findings in `docs/RESEARCH_*.md`
3. Identify what they do well, what they do poorly
4. Design our approach taking the best from each
5. THEN implement

### Research Done So Far:
- [x] Matching Engine: studied gitbitex-new (Bybit core)
- [x] Market Transparency: studied Kraken PoR, Binance zk-SNARKs, Bybit liquidation feed
- [x] API Design: studied Binance/Bybit/OKX kline+ticker formats
- [x] UI/UX: studied Bybit/Binance trading terminal layouts
- [x] Market Maker: studied Bybit MM program, VPIN, Kyle's Lambda
- [x] Accounting: studied TigerBeetle, pgledger, Stripe Ledger, Peatio

### Research Still Needed:
- [ ] Order matching: study LMAX Disruptor pattern (exchange-core)
- [ ] Cross-margin engine: study Bybit UTA internals in depth
- [ ] Liquidation engine: study Bybit/Binance liquidation cascading prevention
- [ ] Index price: study exact weight formulas from Binance/Bybit
- [ ] WebSocket protocol: study Binance/Bybit WS message formats exactly
- [ ] Mobile app architecture: study how Bybit/Binance mobile apps work
- [ ] Compliance: study MiCA technical requirements for VASP
- [ ] Security: study exchange hack post-mortems (Mt Gox, Bitfinex, FTX)
