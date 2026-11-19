# Security Policy

Exchange handles financial logic and real-money risk. We take security seriously
and appreciate responsible disclosure.

## Reporting a vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Report privately through one of:

- GitHub's private vulnerability reporting — preferred. Use the **"Report a
  vulnerability"** button on the repository's **Security** tab.
- Email: `[SECURITY CONTACT EMAIL]`

Please include:

- A description of the vulnerability and its impact
- Steps to reproduce, or a proof of concept
- Affected component(s) and the version or commit

## What to expect

- Acknowledgement of your report within 3 business days
- An assessment and a severity rating
- Coordinated disclosure once a fix is available — we will credit you unless you
  prefer to remain anonymous

## Areas of particular interest

- Authentication, session handling, and API key security
- Order placement, matching, and settlement integrity
- Margin, liquidation, and funding calculations
- Balance and ledger correctness (any path that could create or destroy value)
- Compliance controls (KYC/AML bypass)
- Withdrawal and custody flows

## Out of scope

- Findings in dependencies without a demonstrated impact on this project
- Reports from automated scanners without a working proof of concept
- Issues requiring physical access to a machine or a compromised user device

## A note on project status

This is a reference implementation and has not undergone an independent security
audit. Do not deploy it with real customer funds without your own audit and the
appropriate regulatory authorisation.
