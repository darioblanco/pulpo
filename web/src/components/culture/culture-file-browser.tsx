import { useState, useEffect, useCallback } from 'react';
import { listCultureFiles, readCultureFile } from '@/api/client';
import type { CultureFileEntry } from '@/api/types';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Skeleton } from '@/components/ui/skeleton';
import { FileText, FolderOpen, ArrowLeft } from 'lucide-react';
import { Button } from '@/components/ui/button';

export function CultureFileBrowser() {
  const [files, setFiles] = useState<CultureFileEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [fileContent, setFileContent] = useState<string | null>(null);
  const [fileLoading, setFileLoading] = useState(false);

  const fetchFiles = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listCultureFiles();
      setFiles(data.files);
      setError(null);
    } catch {
      setError('Failed to load culture files');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchFiles();
  }, [fetchFiles]);

  async function handleFileClick(path: string) {
    setSelectedFile(path);
    setFileLoading(true);
    try {
      const data = await readCultureFile(path);
      setFileContent(data.content);
    } catch {
      setFileContent('Failed to load file content');
    } finally {
      setFileLoading(false);
    }
  }

  function handleBack() {
    setSelectedFile(null);
    setFileContent(null);
  }

  if (loading) {
    return (
      <div data-testid="file-browser-loading" className="space-y-2">
        <Skeleton className="h-8 w-full" />
        <Skeleton className="h-8 w-full" />
        <Skeleton className="h-8 w-full" />
      </div>
    );
  }

  if (error) {
    return (
      <p className="py-8 text-center text-destructive" data-testid="file-browser-error">
        {error}
      </p>
    );
  }

  if (selectedFile) {
    return (
      <div data-testid="file-viewer">
        <div className="mb-3 flex items-center gap-2">
          <Button variant="ghost" size="sm" onClick={handleBack} data-testid="back-btn">
            <ArrowLeft className="mr-1 h-4 w-4" />
            Back
          </Button>
          <span className="font-mono text-sm text-muted-foreground">{selectedFile}</span>
        </div>
        {fileLoading ? (
          <Skeleton className="h-64 w-full" />
        ) : (
          <ScrollArea className="h-[calc(100vh-16rem)]">
            <pre className="whitespace-pre-wrap rounded-md bg-muted p-4 font-mono text-sm">
              {fileContent}
            </pre>
          </ScrollArea>
        )}
      </div>
    );
  }

  if (files.length === 0) {
    return (
      <p className="py-8 text-center text-muted-foreground" data-testid="file-browser-empty">
        No culture files yet.
      </p>
    );
  }

  return (
    <div data-testid="file-browser-tree">
      <ScrollArea className="h-[calc(100vh-14rem)]">
        <div className="space-y-0.5">
          {files.map((entry) => (
            <FileEntryRow
              key={entry.path}
              entry={entry}
              onClick={entry.is_dir ? undefined : () => handleFileClick(entry.path)}
            />
          ))}
        </div>
      </ScrollArea>
    </div>
  );
}

function FileEntryRow({ entry, onClick }: { entry: CultureFileEntry; onClick?: () => void }) {
  const depth = entry.path.split('/').length - 1;
  const name = entry.path.split('/').pop() ?? entry.path;

  return (
    <button
      type="button"
      className={`flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm ${
        onClick ? 'cursor-pointer hover:bg-muted' : 'cursor-default text-muted-foreground'
      }`}
      style={{ paddingLeft: `${depth * 16 + 8}px` }}
      onClick={onClick}
      disabled={!onClick}
      data-testid={entry.is_dir ? 'dir-entry' : 'file-entry'}
    >
      {entry.is_dir ? (
        <FolderOpen className="h-4 w-4 shrink-0 text-amber-500" />
      ) : (
        <FileText className="h-4 w-4 shrink-0 text-blue-400" />
      )}
      <span className="truncate font-mono">{name}</span>
    </button>
  );
}
