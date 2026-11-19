import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { ProjectCard } from './components/ProjectCard';
import { CommitForm } from './components/CommitForm';
import { CommitmentTable } from './components/CommitmentTable';
import { getProjects, getMyCommitments } from './api';
import type { LaunchpadProject, LaunchpadCommitment } from './types';
import styles from './LaunchpadPage.module.css';

const TABS: TabDef[] = [
  { key: 'projects', label: 'Projects' },
  { key: 'commitments', label: 'My Commitments' },
];

export default function LaunchpadPage() {
  const [tab, setTab] = useState('projects');
  const [projects, setProjects] = useState<LaunchpadProject[]>([]);
  const [projectsLoading, setProjectsLoading] = useState(false);
  const [commitments, setCommitments] = useState<LaunchpadCommitment[]>([]);
  const [commitmentsLoading, setCommitmentsLoading] = useState(false);
  const [selected, setSelected] = useState<LaunchpadProject | null>(null);

  function loadProjects() {
    setProjectsLoading(true);
    getProjects()
      .then(setProjects)
      .catch(() => setProjects([]))
      .finally(() => setProjectsLoading(false));
  }

  function loadCommitments() {
    setCommitmentsLoading(true);
    getMyCommitments()
      .then(setCommitments)
      .catch(() => setCommitments([]))
      .finally(() => setCommitmentsLoading(false));
  }

  useEffect(() => {
    if (tab === 'projects') loadProjects();
    else loadCommitments();
  }, [tab]);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Launchpad</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'projects' && (
          projectsLoading ? (
            <div className={styles.grid}>
              {Array.from({ length: 6 }, (_, i) => <div key={i} className={styles.skeletonCard} />)}
            </div>
          ) : (
            <div className={styles.grid}>
              {projects.map((p) => (
                <ProjectCard key={p.id} project={p} onSelect={setSelected} />
              ))}
            </div>
          )
        )}

        {tab === 'commitments' && (
          <CommitmentTable commitments={commitments} loading={commitmentsLoading} />
        )}
      </div>

      <CommitForm
        project={selected}
        onClose={() => setSelected(null)}
        onSuccess={loadCommitments}
      />
    </div>
  );
}
