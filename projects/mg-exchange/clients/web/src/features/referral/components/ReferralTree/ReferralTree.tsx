import { useState } from 'react';
import type { ReferralNode } from '../../types';
import styles from './ReferralTree.module.css';

interface NodeItemProps {
  node: ReferralNode;
  depth: number;
}

function NodeItem({ node, depth }: NodeItemProps) {
  const [expanded, setExpanded] = useState(depth === 0);
  const hasChildren = node.children.length > 0;

  return (
    <div className={styles.nodeWrap} style={{ paddingLeft: depth * 20 }}>
      <div className={styles.node}>
        <button
          type="button"
          className={styles.toggle}
          onClick={() => setExpanded((v) => !v)}
          disabled={!hasChildren}
          aria-expanded={expanded}
        >
          {hasChildren ? (expanded ? '▼' : '▶') : '·'}
        </button>
        <span className={styles.nickname}>{node.nickname}</span>
        <span className={styles.meta}>
          Level {node.level} · Joined {new Date(node.joined_at * 1000).toLocaleDateString()}
        </span>
        <span className={styles.commission}>${parseFloat(node.total_commission).toFixed(2)} earned</span>
      </div>
      {expanded && hasChildren && (
        <div className={styles.children}>
          {node.children.map((child) => (
            <NodeItem key={child.user_id} node={child} depth={depth + 1} />
          ))}
        </div>
      )}
    </div>
  );
}

interface ReferralTreeProps {
  nodes: ReferralNode[];
  loading: boolean;
}

export function ReferralTree({ nodes, loading }: ReferralTreeProps) {
  if (loading) return <div className={styles.loading}>Loading...</div>;
  if (nodes.length === 0) return <div className={styles.empty}>No referrals yet</div>;

  return (
    <div className={styles.tree}>
      {nodes.map((node) => (
        <NodeItem key={node.user_id} node={node} depth={0} />
      ))}
    </div>
  );
}
