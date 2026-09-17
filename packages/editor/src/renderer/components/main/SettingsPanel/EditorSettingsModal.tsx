import Modal from '@components/main/Modal/Modal';
import { useTranslation } from '@contexts/useTranslation';
import { useModalPresence } from '@hooks/ui/usePopupPresence';
import EditorSettings from './EditorSettings';

interface EditorSettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  showAlert: (message: string, confirmText?: string) => void;
}

const EditorSettingsModal = ({
  isOpen,
  onClose,
  showAlert,
}: EditorSettingsModalProps) => {
  const presence = useModalPresence(isOpen);
  const { t } = useTranslation();
  if (!presence.mounted) return null;

  return (
    <Modal
      onClick={onClose}
      motionState={presence.state}
      ariaLabel={t('tooltip.settings')}
    >
      <div className="w-[902px] max-w-[calc(100vw-32px)] h-[398px] max-h-[calc(100dvh-122px)] flex flex-col bg-glass-heavy backdrop-glass rounded-modal shadow-elevation-3 overflow-hidden">
        <div className="flex-1 min-h-0 overflow-auto">
          <div className="h-full min-w-[720px]">
            <EditorSettings
              key={presence.cycle}
              embedded
              showAlert={showAlert}
            />
          </div>
        </div>
      </div>
    </Modal>
  );
};

export default EditorSettingsModal;
