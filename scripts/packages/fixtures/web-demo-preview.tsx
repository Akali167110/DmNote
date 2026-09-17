import { useEffect, useLayoutEffect, useRef, useState } from 'react';

const styles = `
.demo-preview-dock{flex-shrink:0;position:relative;border-top:1px solid #ffffff18;background:#18191d}
.demo-preview-dialog{position:relative;inset:auto;margin:0;padding:0;border:0;max-width:none;max-height:none;width:100%;color:inherit;background:#18191d;overflow:hidden}
.demo-preview-dialog:modal{position:fixed;inset:24px;margin:auto;width:calc(100% - 48px);height:calc(100% - 48px);border:1px solid #ffffff24;border-radius:12px;box-shadow:0 24px 100px #0008}
.demo-preview-dialog::backdrop{background:#090a10b8;backdrop-filter:blur(5px)}
.demo-preview-dialog[data-collapsed=true]{display:none}
.demo-preview-body{height:100%;display:flex;flex-direction:column}
.demo-preview-bar{height:40px;flex-shrink:0;display:flex;align-items:center;gap:12px;padding:0 14px}
.demo-preview-title{margin:0;font-size:12px;font-weight:600;color:#ced1d9}
.demo-preview-caption{font-size:11px;color:#858c9a}
.demo-preview-controls{margin-left:auto;display:flex;align-items:center;gap:6px}
.demo-preview-controls .demo-button{padding:2px 9px;font-size:11px}
.demo-preview-stage{position:relative;flex:1;min-height:0;overflow:hidden;background:repeating-conic-gradient(#22242a 0% 25%,#1b1d22 0% 50%) 50%/20px 20px}
.demo-preview-stage iframe{position:absolute;left:50%;top:50%;border:0;transform-origin:top left;background:transparent}
.demo-preview-resize{position:absolute;left:0;right:0;top:-4px;height:8px;z-index:2;cursor:ns-resize;touch-action:none}
.demo-preview-resize::after{content:'';position:absolute;left:calc(50% - 18px);top:3px;width:36px;height:2px;border-radius:2px;background:#737986}
.demo-preview-resize:hover::after,.demo-preview-resize:focus-visible::after{background:#aa96ff;height:3px}
.demo-preview-resize:focus-visible{outline:1px solid #aa96ff;outline-offset:-1px}
@media(max-width:800px){.demo-preview-caption{display:none}.demo-preview-dialog:modal{inset:8px;width:calc(100% - 16px);height:calc(100% - 16px)}}
`;

interface WebDemoPreviewProps {
  documentId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const WebDemoPreview = ({
  documentId,
  open,
  onOpenChange,
}: WebDemoPreviewProps) => {
  const dockRef = useRef<HTMLDivElement>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const frameRef = useRef<HTMLIFrameElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const expandRef = useRef<HTMLButtonElement>(null);
  const dragRef = useRef<{ y: number; height: number } | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [height, setHeight] = useState(220);
  const [maxHeight, setMaxHeight] = useState(500);
  const [resizing, setResizing] = useState(false);
  const [contentSize, setContentSize] = useState({ width: 640, height: 360 });
  const [stageSize, setStageSize] = useState({ width: 640, height: 180 });

  useLayoutEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    // 동일 iframe을 유지한 채 브라우저의 top layer로 확대
    if (expanded && !dialog.matches(':modal')) {
      dialog.close();
      dialog.showModal();
    } else if (!expanded && dialog.matches(':modal')) {
      dialog.close();
      dialog.show();
      expandRef.current?.focus();
    }
  }, [expanded]);

  useEffect(() => {
    const stage = stageRef.current;
    const parent = dockRef.current?.parentElement;
    if (!stage || !parent) return;
    const observer = new ResizeObserver(() => {
      if (stage.clientWidth && stage.clientHeight) {
        setStageSize({ width: stage.clientWidth, height: stage.clientHeight });
      }
      setMaxHeight(Math.max(120, Math.min(600, parent.clientHeight - 220)));
    });
    observer.observe(stage);
    observer.observe(parent);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const receive = (event: MessageEvent) => {
      if (
        event.origin !== location.origin ||
        event.source !== frameRef.current?.contentWindow
      )
        return;
      const data: unknown = event.data;
      if (!data || typeof data !== 'object' || !('type' in data)) return;
      if (data.type === 'demo:preview-escape') {
        setExpanded(false);
      } else if (
        data.type === 'demo:preview-size' &&
        'width' in data &&
        'height' in data &&
        typeof data.width === 'number' &&
        typeof data.height === 'number' &&
        Number.isFinite(data.width) &&
        Number.isFinite(data.height) &&
        data.width > 0 &&
        data.height > 0
      ) {
        setContentSize({ width: data.width, height: data.height });
      }
    };
    window.addEventListener('message', receive);
    return () => window.removeEventListener('message', receive);
  }, []);

  const dockHeight = Math.min(height, maxHeight);
  const scale = Math.max(
    0.01,
    Math.min(
      1,
      (stageSize.width - 32) / contentSize.width,
      (stageSize.height - 24) / contentSize.height,
    ),
  );
  const resize = (next: number) =>
    setHeight(Math.max(120, Math.min(maxHeight, next)));
  return (
    <div className="demo-preview-dock" ref={dockRef}>
      <style>{styles}</style>
      {!open && (
        <div className="demo-preview-bar">
          <h2 className="demo-preview-title">미리보기</h2>
          <span className="demo-preview-caption">
            편집한 디자인과 키 반응 확인
          </span>
          <div className="demo-preview-controls">
            <button
              className="demo-button"
              onClick={() => onOpenChange(true)}
              aria-expanded={false}
              aria-controls="demo-preview-content"
            >
              펼치기 ↑
            </button>
          </div>
        </div>
      )}
      {open && !expanded && (
        <div
          className="demo-preview-resize"
          role="separator"
          aria-label="미리보기 높이"
          aria-orientation="horizontal"
          aria-valuemin={120}
          aria-valuemax={maxHeight}
          aria-valuenow={dockHeight}
          tabIndex={0}
          onPointerDown={(event) => {
            if (event.button !== 0) return;
            event.preventDefault();
            event.currentTarget.setPointerCapture(event.pointerId);
            dragRef.current = { y: event.clientY, height: dockHeight };
            setResizing(true);
          }}
          onPointerMove={(event) => {
            if (dragRef.current)
              resize(
                dragRef.current.height + dragRef.current.y - event.clientY,
              );
          }}
          onPointerUp={(event) => {
            dragRef.current = null;
            setResizing(false);
            if (event.currentTarget.hasPointerCapture(event.pointerId))
              event.currentTarget.releasePointerCapture(event.pointerId);
          }}
          onLostPointerCapture={() => {
            dragRef.current = null;
            setResizing(false);
          }}
          onKeyDown={(event) => {
            if (!['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key))
              return;
            event.preventDefault();
            resize(
              event.key === 'Home'
                ? 120
                : event.key === 'End'
                ? maxHeight
                : dockHeight + (event.key === 'ArrowUp' ? 20 : -20),
            );
          }}
        />
      )}
      <dialog
        ref={dialogRef}
        open
        className="demo-preview-dialog"
        data-collapsed={!open && !expanded}
        aria-label="오버레이 미리보기"
        style={expanded ? undefined : { height: dockHeight }}
        onCancel={(event) => {
          event.preventDefault();
          setExpanded(false);
        }}
        onClick={(event) => {
          if (event.target === event.currentTarget && expanded)
            setExpanded(false);
        }}
      >
        <div className="demo-preview-body" id="demo-preview-content">
          <div className="demo-preview-bar">
            <h2 className="demo-preview-title">미리보기</h2>
            <span className="demo-preview-caption">
              {expanded
                ? 'Esc로 편집 화면에 돌아가기'
                : '상단 경계를 드래그해 높이 조절'}
            </span>
            <div className="demo-preview-controls">
              <span className="demo-preview-caption">
                맞춤 {Math.round(scale * 100)}%
              </span>
              <button
                ref={expandRef}
                className="demo-button"
                onClick={() => setExpanded(!expanded)}
              >
                {expanded ? '축소 보기 ↙' : '확대 보기 ↗'}
              </button>
              {!expanded && (
                <button
                  className="demo-button"
                  onClick={() => onOpenChange(false)}
                  aria-expanded={true}
                  aria-controls="demo-preview-content"
                >
                  접기 ↓
                </button>
              )}
            </div>
          </div>
          <div className="demo-preview-stage" ref={stageRef}>
            <iframe
              ref={frameRef}
              title="오버레이 미리보기"
              src={`/?role=overlay&document=${encodeURIComponent(documentId)}`}
              style={{
                width: contentSize.width,
                height: contentSize.height,
                transform: `translate(${(-contentSize.width * scale) / 2}px, ${
                  (-contentSize.height * scale) / 2
                }px) scale(${scale})`,
                pointerEvents: resizing ? 'none' : undefined,
              }}
            />
          </div>
        </div>
      </dialog>
    </div>
  );
};

export default WebDemoPreview;
