export interface SocialComment {
  id: string;
  author: string;
  body: string;
  created_at: number;
}

export interface SocialPost {
  id: string;
  author: string;
  body: string;
  likes: number;
  comments: number;
  comment_list?: SocialComment[];
  created_at: number;
}

export interface TraderProfile {
  user_id: string;
  username: string;
  bio: string;
  win_rate: string;
  roi_30d: string;
  total_pnl: string;
  followers: number;
  trades: number;
}
