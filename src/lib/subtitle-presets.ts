// 与 Rust timeline.rs / subtitle.rs 保持一致的 8 套花字预设，供 Agent replace_text_tracks 与前端预览共用
export type SubtitlePresetId =
  | 'classic_stroke'
  | 'douyin_bold'
  | 'variety_flower'
  | 'news_bar'
  | 'karaoke_highlight'
  | 'bubble_pop'
  | 'impact_outline'
  | 'minimal_clean'

export type SubtitlePreset = {
  presetId: SubtitlePresetId
  name: string
  description: string
  // 与 TextCue.style 子集对齐，可直接合并到 replace_text_tracks 的 style
  style: {
    color: string
    strokeColor: string | null
    strokeWidth: number
    shadow: boolean
    backgroundColor: string | null
  }
  templateId: string
}

export const SUBTITLE_PRESETS: SubtitlePreset[] = [
  { presetId: 'classic_stroke', name: '经典描边', description: '白字黑描边，通用兜底', style: { color: '#FFFFFF', strokeColor: '#000000', strokeWidth: 6, shadow: true, backgroundColor: null }, templateId: 'subtitle_safe' },
  { presetId: 'douyin_bold', name: '抖音黄边', description: '抖音爆款黄描边白字', style: { color: '#FFFFFF', strokeColor: '#FFD400', strokeWidth: 9, shadow: true, backgroundColor: null }, templateId: 'subtitle_douyin' },
  { presetId: 'variety_flower', name: '综艺花字', description: '橙描边黄字，综艺感', style: { color: '#FFEB3B', strokeColor: '#FF4D00', strokeWidth: 7, shadow: true, backgroundColor: null }, templateId: 'subtitle_variety' },
  { presetId: 'news_bar', name: '新闻条', description: '底部半透明黑条', style: { color: '#FFFFFF', strokeColor: null, strokeWidth: 0, shadow: false, backgroundColor: '#CC000000' }, templateId: 'subtitle_newsbar' },
  { presetId: 'karaoke_highlight', name: '逐字高亮', description: '唱词逐字变色由前端 jassub 驱动', style: { color: '#FFFFFF', strokeColor: '#000000', strokeWidth: 5, shadow: false, backgroundColor: null }, templateId: 'subtitle_karaoke' },
  { presetId: 'bubble_pop', name: '气泡弹跳', description: '黄底黑字气泡', style: { color: '#0F172A', strokeColor: null, strokeWidth: 0, shadow: false, backgroundColor: '#FFE600' }, templateId: 'subtitle_bubble' },
  { presetId: 'impact_outline', name: '冲击描边', description: '青色描边，科技感', style: { color: '#FFFFFF', strokeColor: '#00E5FF', strokeWidth: 8, shadow: true, backgroundColor: null }, templateId: 'subtitle_impact' },
  { presetId: 'minimal_clean', name: '极简白字', description: '无描边，干净字幕', style: { color: '#F8FAFC', strokeColor: null, strokeWidth: 0, shadow: false, backgroundColor: null }, templateId: 'subtitle_safe' },
]

export function presetById(id: string): SubtitlePreset | undefined {
  return SUBTITLE_PRESETS.find(p => p.presetId === id)
}
