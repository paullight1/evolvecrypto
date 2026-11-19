# EvolveCrypto — Five Exchange Candidates

This repository holds **five independent upstream source repositories** as Git submodules, pinned to specific commits. They are not merged or approved for production funds.

| Folder | Original project | Model |
| --- | --- | --- |
| `projects/exclip` | [ExClip](https://github.com/ColdAI-org/exclip) | D: cross-chain swaps |
| `projects/uswap-zero` | [uSwap Zero](https://github.com/uSwapExchange/zero) | D: crypto swaps |
| `projects/mg-exchange` | [MG Exchange](https://github.com/MG-Trading-Terminal/mg-exchange) | B: spot/futures exchange |
| `projects/currency-exchange-platform` | [Currency Exchange Platform](https://github.com/imranbru99/currency-exchange-platform-p2p) | A: fiat/crypto exchange desk |
| `projects/cryplio` | [Cryplio](https://github.com/parvejhossain55/cryplio) | C: P2P marketplace |

## Clone all projects

```bash
git clone --recurse-submodules https://github.com/paullight1/evolvecrypto.git
cd evolvecrypto
git submodule status
```

For an existing clone: `git submodule update --init --recursive`.

Submodules are **references to the complete original repositories**, not copies of their files embedded in the parent Git tree. Git downloads each project's source and history when submodules are initialized.

## Evaluation

Keep projects isolated. Compare real working features, source-code licences, deployment requirements, accounting and custody security, fiat integrations, compliance, test coverage, and mobile usability. Do not combine code or connect live customer funds until a project has passed an independent security review and regulatory assessment.

MG Exchange uses AGPL-3.0, which can require making modified network-served source available; Cryplio has no verified reuse licence. Check upstream licences before commercial reuse.
