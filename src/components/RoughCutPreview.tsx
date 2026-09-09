// 粗剪预览与候选面板：始终与对话并排，试选画面和已保存的整片预览分开呈现。
import { useEffect, useRef, useState } from 'react'
import { convertFileSrc } from '@tauri-apps/api/core'
import type { ArtifactWorkspaceController } from '../hooks/useArtifactWorkspaceController'
import type { ShotReplacementController } from '../hooks/useShotReplacementController'

type Props = {
  model: { artifact: ArtifactWorkspaceController['model']; replacement: ShotReplacementController['model']; agentBusy: boolean }
  actions: { artifact: ArtifactWorkspaceController['actions']; replacement: ShotReplacementController['actions'] }
}

export function RoughCutPreview({ model: { artifact, replacement, agentBusy }, actions }: Props) {
  const video = useRef<HTMLVideoElement>(null)
  const pendingDialog = useRef<HTMLDialogElement>(null)
  const [playhead, setPlayhead] = useState(0)
  const { shot, prepared, phase, recommendations, selectedId } = replacement
  const busy = phase === 'saving' || phase === 'rendering' || artifact.busy.renderingPreview || artifact.busy.creatingJianyingDraft || agentBusy
  const clips = artifact.timeline?.clips ?? []
  const currentClip = clips.find((clip) => playhead >= clip.timelineStartMs && playhead < clip.timelineEndMs) ?? clips[0]
  const stalePreview = artifact.preview && artifact.preview.timelineVersionId !== artifact.timeline?.id
  const selected = recommendations?.candidates.find((candidate) => candidate.assetId === selectedId)
  useEffect(() => {
    if (replacement.pending) pendingDialog.current?.showModal()
    else pendingDialog.current?.close()
  }, [replacement.pending])
  return (
    <section className="rough-preview" aria-label="粗剪预览">
      <header className="preview-heading">
        <div><span className="eyebrow">{shot ? '选择替换画面' : 'ROUGH CUT'}</span><h2>{shot ? `替换镜头 ${String(shot.shotIndex).padStart(2, '0')}` : '视频预览'}</h2></div>
        <span className="format-label">{shot ? `${((shot.timelineEndMs - shot.timelineStartMs) / 1000).toFixed(1)} 秒 · 时长保持不变` : '9:16 · 1080p 草稿'}</span>
      </header>
      {shot ? (
        <div className="replacement-body">
          <div className="candidate-preview">
            <div className="candidate-video">
              {prepared ? <video key={prepared.previewPath} src={convertFileSrc(prepared.previewPath)} controls autoPlay muted loop playsInline /> : <span>{phase === 'preparing' ? '正在准备候选画面…' : '选择一个推荐镜头'}</span>}
            </div>
            <div><span className="eyebrow">画面试选 · 尚未保存</span><h3>{selected?.displayName ?? '为这一段找到更合适的画面'}</h3><p>{recommendations?.beatPurpose}</p><small>仅预览候选画面。保存后更新整片预览，保留配音、字幕与音乐。</small></div>
          </div>
          <div className="candidate-heading"><strong>推荐镜头</strong><span>{recommendations ? `${recommendations.candidates.length} 个候选` : '读取中…'}</span></div>
          {recommendations && !recommendations.saved && <div className="preview-empty"><p>这版粗剪尚未保存推荐镜头。</p><button className="outline-button" onClick={actions.replacement.generate}>生成推荐镜头</button></div>}
          {recommendations?.saved && !recommendations.candidates.length && <p className="workspace-notice">当前没有可用候选。请补充素材，或通过对话调整这一段。</p>}
          <div className="candidate-grid">
            {recommendations?.candidates.map((candidate, index) => (
              <button key={candidate.assetId} className={`candidate-card ${candidate.assetId === selectedId ? 'selected' : ''}`} disabled={busy || phase === 'preparing' || candidate.current || Boolean(candidate.unavailableReason)} onClick={() => actions.replacement.select(candidate.assetId)} aria-pressed={candidate.assetId === selectedId} title={candidate.unavailableReason ?? candidate.displayName}>
                <div className="candidate-image">{candidate.thumbnailPath ? <img src={convertFileSrc(candidate.thumbnailPath)} alt="" /> : <span>暂无缩略图</span>}<b>{String(index + 1).padStart(2, '0')}</b><small>{candidate.current ? '当前镜头' : candidate.usedInTimeline ? '其他镜头已用' : `${((candidate.durationMs ?? 0) / 1000).toFixed(1)}s`}</small></div>
                <span>{candidate.displayName}</span>{candidate.unavailableReason && <small>{candidate.unavailableReason}</small>}
              </button>
            ))}
          </div>
        </div>
      ) : (
        <>
          <div className="rough-player">
            {artifact.preview ? <video ref={video} key={`${artifact.preview.previewPath}:${artifact.previewNonce}`} src={convertFileSrc(artifact.preview.previewPath)} controls playsInline onTimeUpdate={(event) => setPlayhead(event.currentTarget.currentTime * 1000)} /> : <div className="preview-empty"><span className="empty-play">▷</span><h3>{agentBusy ? '正在制作你的第一版粗剪' : '你的第一版，从对话开始'}</h3><p>导入素材，告诉我想剪什么。<br />完成后在这里预览并微调镜头。</p>{artifact.storyboard && <button className="outline-button" disabled={busy || artifact.busy.creatingTimeline} onClick={artifact.timeline ? actions.artifact.renderPreview : actions.artifact.createTimeline}>{artifact.timeline ? '生成预览' : '生成粗剪'}</button>}</div>}
          </div>
          <div className="shot-strip" aria-label="粗剪镜头">
            {clips.map((clip) => <button key={clip.shotIndex} className={clip.shotIndex === currentClip?.shotIndex ? 'selected' : ''} onClick={() => { setPlayhead(clip.timelineStartMs); if (video.current) video.current.currentTime = clip.timelineStartMs / 1000 }} aria-label={`定位镜头 ${clip.shotIndex}`}><b>{String(clip.shotIndex).padStart(2, '0')}</b><span>{((clip.timelineEndMs - clip.timelineStartMs) / 1000).toFixed(1)}s</span></button>)}
          </div>
          <div className="preview-tools"><div><button className="outline-button" disabled={!currentClip || busy || (currentClip.clipKind ?? 'source') !== 'source'} onClick={() => { if (currentClip) actions.replacement.open(currentClip) }}>替换当前镜头</button><button className="text-button" disabled={!replacement.canUndo || busy} onClick={actions.replacement.undo}>撤销</button><button className="text-button" disabled={!replacement.canRedo || busy} onClick={actions.replacement.redo}>重做</button></div>{artifact.timeline && <button className="text-button" disabled={busy} onClick={actions.artifact.renderPreview}>更新预览</button>}</div>
        </>
      )}
      {(replacement.notice || stalePreview || artifact.jianyingNotice) && <div className="workspace-notice" role="status">{replacement.notice && <p>{replacement.notice}</p>}{stalePreview && !replacement.notice && <p>当前画面为上一版，请更新预览以查看已保存的修改。</p>}{artifact.jianyingNotice && <p>{artifact.jianyingNotice}</p>}</div>}
      <footer className="preview-footer"><span>{shot ? '保存后才会修改粗剪' : '粗剪完成后，在剪映继续精修'}</span>{shot ? <div><button className="outline-button" disabled={phase === 'saving'} onClick={actions.replacement.cancel}>取消</button><button className="primary-button" disabled={!prepared || phase !== 'idle' || busy} onClick={actions.replacement.save}>{phase === 'saving' ? '保存中…' : '保存修改'}</button></div> : <button className="primary-button" disabled={!artifact.timeline || busy} onClick={() => actions.replacement.requestAction((timeline) => actions.artifact.createJianyingDraft(timeline))}>{artifact.busy.creatingJianyingDraft ? '正在生成草稿…' : '生成剪映草稿 ↗'}</button>}</footer>
      <dialog ref={pendingDialog} aria-labelledby="pending-title" className="pending-dialog" onCancel={actions.replacement.keepEditing}><h3 id="pending-title">还有未保存的镜头修改</h3><p>先保存这次试选，还是放弃后继续？</p><button className="primary-button" disabled={!prepared || phase !== 'idle'} onClick={actions.replacement.saveAndContinue}>保存并继续</button><button className="outline-button" onClick={actions.replacement.discardAndContinue}>放弃并继续</button><button className="text-button" onClick={actions.replacement.keepEditing}>继续编辑</button></dialog>
    </section>
  )
}
