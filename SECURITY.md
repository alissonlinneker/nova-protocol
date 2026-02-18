# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

The NOVA Protocol team takes security seriously. If you discover a security vulnerability, please follow the responsible disclosure process below.

### How to Report

1. **Do NOT open a public issue.** Security vulnerabilities must be reported privately.

2. **Email us at**: security@nova-protocol.org

3. **Include the following information**:
   - Description of the vulnerability
   - Steps to reproduce the issue
   - Potential impact assessment
   - Suggested fix (if any)

### What to Expect

- **Acknowledgment**: We will acknowledge your report within 48 hours.
- **Assessment**: We will assess the severity and impact within 5 business days.
- **Fix Timeline**: Critical vulnerabilities will be patched within 7 days. High-severity issues within 14 days.
- **Disclosure**: We will coordinate disclosure with you. We ask for a 90-day disclosure window.

### Scope

The following are in scope for security reports:

- **Protocol crate** (`protocol/`): Cryptographic primitives, identity, ZKP, consensus
- **Node binary** (`node/`): API endpoints, P2P networking, validator logic
- **Smart contracts** (`contracts/`): Credit escrow, dispute resolution, token factory
- **SDKs** (`sdk/`): TypeScript and Python client libraries

### Out of Scope

- Web application UI bugs (unless they lead to credential exposure)
- Denial of service via excessive API calls (rate limiting is expected at the infrastructure level)
- Issues in third-party dependencies (report these upstream, but let us know)

### Recognition

We maintain a security hall of fame for responsible disclosures. With your permission, we will publicly credit you for your contribution to the security of the NOVA Protocol.

## Security Design

For details on the cryptographic architecture and threat model, see the [Security section](README.md#security) of the README.
