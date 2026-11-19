import { useState } from 'react';
import { ConfirmDialog } from '@shared/ui/ConfirmDialog';
import { toast } from '@shared/ui/Toast';
import { joinCompetition } from '../../api';
import type { Competition } from '../../types';

interface JoinFormProps {
  competition: Competition | null;
  onClose: () => void;
  onSuccess: () => void;
}

export function JoinForm({ competition, onClose, onSuccess }: JoinFormProps) {
  const [joining, setJoining] = useState(false);

  async function handleJoin() {
    if (!competition) return;
    setJoining(true);
    try {
      await joinCompetition({ competition_id: competition.id });
      toast.success(`Joined ${competition.name}`);
      onSuccess();
      onClose();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to join');
    } finally {
      setJoining(false);
    }
  }

  return (
    <ConfirmDialog
      open={competition !== null && !joining}
      title={`Join ${competition?.name ?? 'Competition'}`}
      message={`You are about to join this competition. Your trading performance will be tracked from the time you join.`}
      confirmText="Join Competition"
      onConfirm={() => { void handleJoin(); }}
      onCancel={onClose}
    />
  );
}
