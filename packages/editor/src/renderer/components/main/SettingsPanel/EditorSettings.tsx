import { useRef, useState } from 'react';
import { useTranslation } from '@contexts/useTranslation';
import type { SupportedLocale } from '@contexts/I18nContextDef';
import { useSettingsStore } from '@stores/useSettingsStore';
import { useLenis } from '@hooks/useLenis';
import { useDeferredHover } from '@hooks/ui/useDeferredHover';
import { useRetainedWhileOpen } from '@hooks/ui/useRetainedValue';
import { useSingleFlightAction } from '@hooks/useSingleFlightAction';
import { settingsApi } from '@api/modules/app/settingsApi';
import { cssApi } from '@api/modules/resources/cssApi';
import { jsApi } from '@api/modules/plugin/jsApi';
import { getPluginDisplayName } from '@utils/plugin/components/pluginUtils';
import Dropdown from '@components/main/common/dropdown/Dropdown';
import ReloadButton from '@components/main/common/ReloadButton';
import {
  SettingCard,
  SettingRow,
  SettingToggleRow,
} from '@components/main/common/SettingRow';
import { PluginDataDeleteModal } from '@components/main/Modal/content/dialogs/PluginDataDeleteModal';
import SettingsPreview from './SettingsPreview';
import SettingsSidePanel from './SettingsSidePanel';
import CssPanelContent from './CssPanelContent';
import PluginsPanelContent from './PluginsPanelContent';
import { EDITOR_SETTINGS_PREVIEW_CLIPS } from './editorSettingsPreviewClips';
import { FILL_INTERACTIVE_CLASS } from './panelChrome';
import {
  createSettingsPluginLifecycleController,
  type PluginToDelete,
} from './settingsPluginLifecycleController';

const LANGUAGE_OPTIONS = [
  { value: 'ko', label: '한국어' },
  { value: 'en', label: 'English' },
  { value: 'zh-cn', label: '简体中文' },
  { value: 'zh-Hant', label: '繁體中文' },
  { value: 'ru', label: 'Русский' },
];

const ACTION_BUTTON_CLASS = `inline-flex items-center h-[23px] px-[10px] rounded-md text-body transition-colors duration-fast ${FILL_INTERACTIVE_CLASS}`;

interface EditorSettingsProps {
  showAlert: (message: string, confirmText?: string) => void;
  /** 상위 모달이 키보드와 포커스 수명을 소유하는 경우 */
  embedded?: boolean;
}

const EditorSettings = ({
  showAlert,
  embedded = false,
}: EditorSettingsProps) => {
  const { t, i18n } = useTranslation();
  const {
    noteEffect,
    setNoteEffect,
    keyCounterEnabled,
    setKeyCounterEnabled,
    useCustomCSS,
    setUseCustomCSS,
    customCSSPath,
    customCSSContent,
    useCustomJS,
    setUseCustomJS,
    jsPlugins,
    language,
    setLanguage,
  } = useSettingsStore();
  const { scrollContainerRef } = useLenis();
  const [hoveredKey, hoverPreview] = useDeferredHover();
  const [activePanel, setActivePanel] = useState<'css' | 'plugins' | null>(
    null,
  );
  const [cssHistoryCount, setCssHistoryCount] = useState(0);
  const [isDataDeleteModalOpen, setDataDeleteModalOpen] = useState(false);
  const [pluginToDelete, setPluginToDelete] = useState<PluginToDelete | null>(
    null,
  );
  const dataDeleteModalOpen = isDataDeleteModalOpen && !!pluginToDelete;
  const shownPluginToDelete = useRetainedWhileOpen(
    dataDeleteModalOpen,
    pluginToDelete,
  );
  const [isReloadingPlugins, setIsReloadingPlugins] = useState(false);
  const [isAddingPlugins, setIsAddingPlugins] = useState(false);
  const [pendingPluginId, setPendingPluginId] = useState<string | null>(null);
  const reloadingPluginsRef = useRef(false);
  const addingPluginsRef = useRef(false);
  const pendingPluginRef = useRef<string | null>(null);
  const removingPluginRef = useRef<string | null>(null);
  // 컨트롤러는 ref를 캡처만 하고, 실제 값은 이벤트 핸들러에서 읽는다
  // eslint-disable-next-line react-hooks/refs
  const plugins = createSettingsPluginLifecycleController({
    t,
    showAlert,
    jsPlugins,
    setPluginToDelete,
    setDataDeleteModalOpen,
    setIsReloadingPlugins,
    setIsAddingPlugins,
    setPendingPluginId,
    reloadingPluginsRef,
    addingPluginsRef,
    pendingPluginRef,
    removingPluginRef,
  });

  const toggle = async (
    current: boolean,
    setValue: (value: boolean) => void,
    save: (value: boolean) => Promise<unknown>,
  ): Promise<void> => {
    setValue(!current);
    try {
      await save(!current);
    } catch (error) {
      setValue(current);
      console.error('Failed to update editor settings', error);
      showAlert(t('common.saveFailed'));
    }
  };

  const { run: handleLanguageChange, pending: languagePending } =
    useSingleFlightAction(async (value: string) => {
      try {
        await i18n.changeLanguage(value as SupportedLocale);
        setLanguage(value);
      } catch (error) {
        console.error('Failed to change language', error);
        showAlert(t('common.saveFailed'));
      }
    });

  const panelCountBadge = (count: number) =>
    count === 1
      ? t('settings.panelCountBadgeOne')
      : t('settings.panelCountBadge', { count: String(count) });

  return (
    <div className="relative w-full h-full">
      <div
        ref={scrollContainerRef}
        className={`settings-content-scroll w-full h-full flex flex-col py-[12px] px-[12px] gap-[12px] overflow-y-auto${
          embedded ? '' : ' bg-panel'
        }`}
      >
        <div className="flex flex-row gap-[12px]">
          <div className="flex flex-col gap-[12px] w-[348px]">
            <SettingCard>
              <SettingToggleRow
                commitStrategy="after-paint"
                label={t('settings.noteEffect')}
                checked={noteEffect}
                onToggle={() =>
                  void toggle(noteEffect, setNoteEffect, (value) =>
                    settingsApi.update({ noteEffect: value }),
                  )
                }
                onMouseEnter={() => hoverPreview('noteEffect')}
                onMouseLeave={() => hoverPreview(null)}
              />
              <SettingToggleRow
                commitStrategy="after-paint"
                label={t('settings.keyCounter')}
                checked={keyCounterEnabled}
                onToggle={() =>
                  void toggle(
                    keyCounterEnabled,
                    setKeyCounterEnabled,
                    (value) => settingsApi.update({ keyCounterEnabled: value }),
                  )
                }
                onMouseEnter={() => hoverPreview('keyCounter')}
                onMouseLeave={() => hoverPreview(null)}
              />
            </SettingCard>
            <SettingCard>
              <SettingRow
                label={t('settings.customCSSLabel')}
                onMouseEnter={() => hoverPreview('customCSS')}
                onMouseLeave={() => hoverPreview(null)}
              >
                <button
                  onClick={() =>
                    setActivePanel((current) =>
                      current === 'css' ? null : 'css',
                    )
                  }
                  className={ACTION_BUTTON_CLASS}
                >
                  {t('settings.manageCss')}
                </button>
              </SettingRow>
              <SettingRow
                label={t('settings.customJSLabel')}
                onMouseEnter={() => hoverPreview('customJS')}
                onMouseLeave={() => hoverPreview(null)}
              >
                <div className="flex flex-row gap-[6px]">
                  <ReloadButton
                    onClick={plugins.handleReloadPlugins}
                    disabled={!plugins.canReloadPlugins}
                    busy={isReloadingPlugins}
                    title={t('settings.reloadPlugins')}
                  />
                  <button
                    onClick={() =>
                      setActivePanel((current) =>
                        current === 'plugins' ? null : 'plugins',
                      )
                    }
                    className={ACTION_BUTTON_CLASS}
                  >
                    {t('settings.managePlugins')}
                  </button>
                </div>
              </SettingRow>
            </SettingCard>
            <SettingCard>
              <SettingRow label={t('settings.language')}>
                <Dropdown
                  options={LANGUAGE_OPTIONS}
                  value={language}
                  onChange={handleLanguageChange}
                  disabled={languagePending}
                  placeholder={t('settings.selectLanguage')}
                  align="right"
                />
              </SettingRow>
            </SettingCard>
          </div>
        </div>
      </div>
      <div
        className={
          'absolute top-[12px] right-[12px] bottom-[12px] left-[372px] bg-fill-faint rounded-surface overflow-hidden' +
          (activePanel ? '' : ' pointer-events-none')
        }
      >
        {activePanel ? (
          <SettingsSidePanel
            embedded={embedded}
            activePanel={activePanel}
            onClose={() => setActivePanel(null)}
            pages={[
              {
                key: 'plugins',
                title: t('settings.managePluginsTitle'),
                headerBadge: panelCountBadge(jsPlugins.length),
                content: (
                  <PluginsPanelContent
                    plugins={jsPlugins}
                    useCustomJS={useCustomJS}
                    onToggleCustomJS={() =>
                      void toggle(useCustomJS, setUseCustomJS, jsApi.toggle)
                    }
                    onAdd={plugins.handleAddPlugins}
                    onToggle={plugins.handlePluginToggle}
                    onRemove={plugins.handlePluginRemove}
                    isAdding={isAddingPlugins}
                    isPluginActionPending={pendingPluginId !== null}
                    onClose={() => setActivePanel(null)}
                  />
                ),
              },
              {
                key: 'css',
                title: t('settings.manageCssTitle'),
                headerBadge: panelCountBadge(cssHistoryCount),
                content: (
                  <CssPanelContent
                    useCustomCSS={useCustomCSS}
                    customCSSPath={customCSSPath}
                    customCSSContent={customCSSContent}
                    onToggleCustomCSS={() =>
                      void toggle(useCustomCSS, setUseCustomCSS, cssApi.toggle)
                    }
                    showAlert={showAlert}
                    onClose={() => setActivePanel(null)}
                    onHistoryCountChange={setCssHistoryCount}
                  />
                ),
              },
            ]}
          />
        ) : (
          <SettingsPreview
            hoveredKey={hoveredKey}
            clips={EDITOR_SETTINGS_PREVIEW_CLIPS}
          />
        )}
      </div>
      {shownPluginToDelete && (
        <PluginDataDeleteModal
          isOpen={dataDeleteModalOpen}
          onClose={() => {
            setDataDeleteModalOpen(false);
            setPluginToDelete(null);
          }}
          onConfirm={(withData) =>
            withData
              ? plugins.removePluginWithData(shownPluginToDelete.id)
              : plugins.removePluginOnly(shownPluginToDelete.id)
          }
          pluginName={getPluginDisplayName(shownPluginToDelete.name)}
          t={t}
        />
      )}
    </div>
  );
};

export default EditorSettings;
