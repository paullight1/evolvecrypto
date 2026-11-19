import { useState } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import styles from './InfoPage.module.css';

const TABS: TabDef[] = [
  { key: 'tos', label: 'Terms of Service' },
  { key: 'privacy', label: 'Privacy Policy' },
  { key: 'aml', label: 'AML Policy' },
];

const CONTENT: Record<string, string> = {
  tos: `Terms of Service

Last updated: January 1, 2026

1. Acceptance of Terms
By accessing or using the Exchange platform, you agree to be bound by these Terms of Service and all applicable laws and regulations. If you do not agree with any of these terms, you are prohibited from using this platform.

2. Eligibility
You must be at least 18 years old and legally permitted to trade cryptocurrency in your jurisdiction. Users from prohibited jurisdictions are not allowed to use this platform.

3. Account Responsibilities
You are responsible for maintaining the confidentiality of your account credentials. You agree to notify us immediately of any unauthorized use of your account.

4. Trading Rules
All trading activity must comply with our fair usage policies. Market manipulation, wash trading, and spoofing are strictly prohibited and may result in account termination.

5. Risk Disclosure
Trading cryptocurrencies involves significant risk of loss. Past performance does not guarantee future results. Only trade with funds you can afford to lose.

6. Fees
Trading fees are disclosed on the Fees page and may be updated with 30 days notice. By continuing to use the platform after fee changes take effect, you accept the new fees.

7. Governing Law
These terms shall be governed by and construed in accordance with applicable laws.`,

  privacy: `Privacy Policy

Last updated: January 1, 2026

1. Information We Collect
We collect information you provide directly (email, name, ID documents for KYC), information generated through your use of the platform (trading history, IP addresses, device information), and information from third parties (blockchain data for compliance).

2. How We Use Information
- To operate and improve the platform
- To verify your identity (KYC/AML compliance)
- To detect and prevent fraud
- To send service communications
- To comply with legal obligations

3. Information Sharing
We do not sell your personal data. We may share information with: regulatory authorities when required by law, service providers who assist in operations, and with your consent for other purposes.

4. Data Security
We implement industry-standard security measures including encryption at rest and in transit, two-factor authentication, and regular security audits.

5. Data Retention
We retain account data for 7 years after account closure as required by financial regulations. You may request deletion of non-required data.

6. Your Rights
You have the right to access, correct, or request deletion of your personal data, subject to legal retention requirements. Contact support to exercise these rights.`,

  aml: `Anti-Money Laundering Policy

Last updated: January 1, 2026

1. Our Commitment
We are committed to preventing money laundering, terrorist financing, and other financial crimes. We comply with all applicable AML/CFT laws and regulations.

2. KYC Requirements
All users must complete identity verification. Enhanced due diligence is required for high-risk profiles, politically exposed persons (PEPs), and large transactions.

3. Transaction Monitoring
We monitor all transactions for suspicious activity using automated systems and manual review. Unusual patterns trigger enhanced scrutiny.

4. Suspicious Activity Reporting
We are required by law to report suspicious transactions to relevant financial intelligence authorities. We cannot inform users when such reports are filed.

5. Prohibited Activities
The platform may not be used for:
- Layering or structuring transactions to avoid reporting
- Processing proceeds from illegal activities
- Financing terrorism or proliferation

6. Sanctions Compliance
We screen all users and transactions against international sanctions lists. Sanctioned individuals and entities are prohibited from using the platform.`,
};

export default function LegalPage() {
  const [tab, setTab] = useState('tos');

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Legal</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} />
      <div className={styles.legalContent} style={{ marginTop: 'var(--pad)' }}>{CONTENT[tab]}</div>
    </div>
  );
}
