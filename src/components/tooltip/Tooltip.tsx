import {
  cloneElement,
  isValidElement,
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type FocusEvent,
  type MouseEvent,
  type ReactElement,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
import './Tooltip.css';

type TooltipProps = {
  content: ReactNode;
  children: ReactNode;
  align?: 'start' | 'center' | 'end';
  className?: string;
};

type DescribedByProps = {
  'aria-describedby'?: string;
};

export function Tooltip({ content, children, align = 'start', className }: TooltipProps) {
  const tooltipId = useId().replace(/:/g, '');
  const rootClassName = ['app-tooltip', className].filter(Boolean).join(' ');
  const rootRef = useRef<HTMLSpanElement | null>(null);
  const bubbleRef = useRef<HTMLSpanElement | null>(null);
  const [isOpen, setIsOpen] = useState(false);
  const [placement, setPlacement] = useState<'top' | 'bottom'>('bottom');
  const [bubbleStyle, setBubbleStyle] = useState<CSSProperties>({});
  const target = isValidElement<DescribedByProps>(children)
    ? cloneElement(children as ReactElement<DescribedByProps>, {
        'aria-describedby': tooltipId,
      })
    : children;

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    function updatePosition() {
      const root = rootRef.current;
      const bubble = bubbleRef.current;
      if (!root || !bubble) {
        return;
      }

      const rect = root.getBoundingClientRect();
      const bubbleWidth = bubble.offsetWidth;
      const bubbleHeight = bubble.offsetHeight;
      const viewportWidth = window.innerWidth;
      const viewportHeight = window.innerHeight;
      const margin = 12;
      const gap = 10;
      const arrowInset = 17;

      const alignedLeft =
        align === 'center'
          ? rect.left + rect.width / 2 - bubbleWidth / 2
          : align === 'end'
            ? rect.right - bubbleWidth
            : rect.left;
      const left = Math.min(
        Math.max(margin, alignedLeft),
        Math.max(margin, viewportWidth - bubbleWidth - margin),
      );

      const shouldPlaceAbove = rect.bottom + gap + bubbleHeight > viewportHeight - margin;
      const top = shouldPlaceAbove ? rect.top - bubbleHeight - gap : rect.bottom + gap;
      const anchorX =
        align === 'center'
          ? rect.left + rect.width / 2
          : align === 'end'
            ? rect.right - arrowInset
            : rect.left + arrowInset;
      const arrowLeft = Math.min(Math.max(16, anchorX - left), Math.max(16, bubbleWidth - 16));

      setPlacement(shouldPlaceAbove ? 'top' : 'bottom');
      setBubbleStyle({
        left,
        top: Math.max(margin, top),
        ['--tooltip-arrow-left' as string]: `${arrowLeft}px`,
      });
    }

    updatePosition();
    window.addEventListener('resize', updatePosition);
    window.addEventListener('scroll', updatePosition, true);
    return () => {
      window.removeEventListener('resize', updatePosition);
      window.removeEventListener('scroll', updatePosition, true);
    };
  }, [align, isOpen]);

  function handleMouseEnter(_event: MouseEvent<HTMLSpanElement>) {
    setIsOpen(true);
  }

  function handleMouseLeave(_event: MouseEvent<HTMLSpanElement>) {
    setIsOpen(false);
  }

  function handleFocus(_event: FocusEvent<HTMLSpanElement>) {
    setIsOpen(true);
  }

  function handleBlur(event: FocusEvent<HTMLSpanElement>) {
    if (event.currentTarget.contains(event.relatedTarget)) {
      return;
    }

    setIsOpen(false);
  }

  return (
    <span
      ref={rootRef}
      className={rootClassName}
      data-align={align}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      onFocus={handleFocus}
      onBlur={handleBlur}
    >
      <span className="app-tooltip-target">{target}</span>
      {typeof document !== 'undefined'
        ? createPortal(
            <span
              ref={bubbleRef}
              id={tooltipId}
              role="tooltip"
              className="app-tooltip-bubble"
              data-open={isOpen ? 'true' : 'false'}
              data-placement={placement}
              style={bubbleStyle}
            >
              {content}
            </span>,
            document.body,
          )
        : null}
    </span>
  );
}
