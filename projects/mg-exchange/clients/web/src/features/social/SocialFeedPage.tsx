import { useState, useEffect } from 'react';
import { Card } from '@shared/ui/Card';
import { Button } from '@shared/ui/Button';
import { Avatar } from '@shared/ui/Avatar';
import { EmptyState } from '@shared/ui/EmptyState';
import { toast } from '@shared/ui/Toast';
import { navigate } from '@app/Router';
import { getFeed, createPost, likePost, getComments, addComment } from './api';
import type { SocialPost, SocialComment } from './types';
import styles from './SocialPage.module.css';

export default function SocialFeedPage() {
  const [posts, setPosts] = useState<SocialPost[]>([]);
  const [loading, setLoading] = useState(true);
  const [newPost, setNewPost] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [expandedComments, setExpandedComments] = useState<Set<string>>(new Set());
  const [commentTexts, setCommentTexts] = useState<Record<string, string>>({});
  const [postComments, setPostComments] = useState<Record<string, SocialComment[]>>({});
  const [loadingComments, setLoadingComments] = useState<Set<string>>(new Set());

  function loadPosts() {
    setLoading(true);
    getFeed()
      .then(setPosts)
      .catch(() => setPosts([]))
      .finally(() => setLoading(false));
  }

  useEffect(() => { loadPosts(); }, []);

  async function handlePost() {
    if (!newPost.trim()) return;
    setSubmitting(true);
    try {
      await createPost(newPost.trim());
      setNewPost('');
      loadPosts();
      toast.success('Posted!');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to post');
    } finally {
      setSubmitting(false);
    }
  }

  async function handleLike(id: string) {
    try {
      await likePost(id);
      setPosts((prev) => prev.map((p) => p.id === id ? { ...p, likes: p.likes + 1 } : p));
    } catch {
      // silent fail
    }
  }

  async function toggleComments(postId: string) {
    const next = new Set(expandedComments);
    if (next.has(postId)) {
      next.delete(postId);
    } else {
      next.add(postId);
      if (!postComments[postId]) {
        setLoadingComments((prev) => new Set(prev).add(postId));
        try {
          const comments = await getComments(postId);
          setPostComments((prev) => ({ ...prev, [postId]: comments }));
        } catch {
          setPostComments((prev) => ({ ...prev, [postId]: [] }));
        } finally {
          setLoadingComments((prev) => { const s = new Set(prev); s.delete(postId); return s; });
        }
      }
    }
    setExpandedComments(next);
  }

  async function handleComment(postId: string) {
    const text = commentTexts[postId]?.trim();
    if (!text) return;
    try {
      await addComment(postId, text);
      setCommentTexts((prev) => ({ ...prev, [postId]: '' }));
      // Reload comments
      const comments = await getComments(postId);
      setPostComments((prev) => ({ ...prev, [postId]: comments }));
      setPosts((prev) => prev.map((p) => p.id === postId ? { ...p, comments: p.comments + 1 } : p));
    } catch {
      toast.error('Failed to add comment');
    }
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Social Feed</h1>

      <Card padding="md">
        <div className={styles.postForm}>
          <textarea
            className={styles.textarea}
            placeholder="Share your trading insights..."
            value={newPost}
            onChange={(e) => setNewPost(e.target.value)}
          />
          <div className={styles.postActions}>
            <Button variant="primary" size="sm" loading={submitting} onClick={() => { void handlePost(); }}>Post</Button>
          </div>
        </div>
      </Card>

      {loading ? (
        <p style={{ color: 'var(--text-2)' }}>Loading feed...</p>
      ) : posts.length === 0 ? (
        <EmptyState
          title="No posts yet"
          description="Be the first to share your trading insights with the community"
        />
      ) : (
        <div className={styles.feed}>
          {posts.map((p) => (
            <div key={p.id} className={styles.post}>
              <div className={styles.postHeader}>
                <button
                  type="button"
                  className={styles.authorLink}
                  onClick={() => navigate(`trader-profile?id=${p.author}`)}
                >
                  <Avatar name={p.author} size="sm" />
                  <span className={styles.postAuthor}>{p.author}</span>
                </button>
                <span className={styles.postTime}>{new Date(p.created_at * 1000).toLocaleString()}</span>
              </div>
              <div className={styles.postBody}>{p.body}</div>
              <div className={styles.postFooter}>
                <button type="button" className={styles.postAction} onClick={() => { void handleLike(p.id); }}>
                  ♥ {p.likes}
                </button>
                <button type="button" className={styles.postAction} onClick={() => { void toggleComments(p.id); }}>
                  💬 {p.comments} {expandedComments.has(p.id) ? '▲' : '▼'}
                </button>
              </div>

              {/* Comment thread */}
              {expandedComments.has(p.id) && (
                <div className={styles.commentThread}>
                  {loadingComments.has(p.id) ? (
                    <p className={styles.commentLoading}>Loading comments...</p>
                  ) : (
                    <>
                      {(postComments[p.id] ?? []).map((c) => (
                        <div key={c.id} className={styles.comment}>
                          <span className={styles.commentAuthor}>{c.author}</span>
                          <span className={styles.commentBody}>{c.body}</span>
                          <span className={styles.commentTime}>{new Date(c.created_at * 1000).toLocaleString()}</span>
                        </div>
                      ))}
                      {(postComments[p.id] ?? []).length === 0 && (
                        <p className={styles.commentEmpty}>No comments yet</p>
                      )}
                      <div className={styles.commentForm}>
                        <input
                          type="text"
                          className={styles.commentInput}
                          placeholder="Write a comment..."
                          value={commentTexts[p.id] ?? ''}
                          onChange={(e) => setCommentTexts((prev) => ({ ...prev, [p.id]: e.target.value }))}
                          onKeyDown={(e) => { if (e.key === 'Enter') { void handleComment(p.id); } }}
                        />
                        <Button variant="ghost" size="sm" onClick={() => { void handleComment(p.id); }}>Reply</Button>
                      </div>
                    </>
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
