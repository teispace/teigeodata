import { ServerCodeBlock } from 'fumadocs-ui/components/codeblock.rsc';
import { Tab, Tabs } from 'fumadocs-ui/components/tabs';
import { output, region, tours } from '@/lib/examples';

/**
 * A region of the example programs in every language, as tabs of the docs' one language group
 * (the reader's pick is kept across pages), with the output every language printed.
 */
export function TourCode({ region: name, title, output: showOutput = true }: { region: string; title?: string; output?: boolean }) {
  return (
    <>
      <Tabs groupId="lang" persist items={tours.map((t) => t.label)}>
        {tours.map((t) => (
          <Tab key={t.label} value={t.label}>
            <ServerCodeBlock
              lang={t.lang}
              code={region(t.file, name)}
              codeblock={{ title: title ?? t.file.split('/').at(-1) }}
            />
          </Tab>
        ))}
      </Tabs>
      {showOutput ? (
        <ServerCodeBlock lang="text" code={output(name)} codeblock={{ title: 'Output' }} />
      ) : null}
    </>
  );
}
